// The export folder as a source of the page: what `testing export --out` wrote, read through the manifest. Every file is
// checked against its sum and the contract before anything of the export is used, so the page never shows half an
// export: a new read replaces the held one only when all of it is good, and a failure leaves the old one in place.
//
// Nothing here reaches the network, and it holds no more than the documents in memory.

import type { DirHandleLike, FileHandleLike } from 'appkit-web/folder';
import type { Fleet, Manifest, RepoDoc } from '../lib/contract';
import type { Capabilities, FleetResult, TestingSource } from '../lib/source';
import validate from './validate.generated.js';

export const MANIFEST = 'testing-export.json';
/** The version of the contract this page knows. */
export const SCHEMA_VERSION = 1;
/** A document is never bigger (`testing.export.MAX_DOC_BYTES`). */
export const MAX_DOC_BYTES = 8 * 1024 * 1024;
/** A fleet is some dozens of repositories: far more than this is another kind of folder. */
export const MAX_FILES = 2000;
/** All documents of one export together: they are held in memory. */
export const MAX_TOTAL_BYTES = 256 * 1024 * 1024;

export type ExportProblem = 'no_manifest' | 'repos_folder' | 'too_new' | 'not_export' | 'hash' | 'missing' | 'too_large' | 'bad_path' | 'invalid';

/** Why an export cannot be used. `detail` is a path of the export or a word of the contract, never the text of a document. */
export class ExportError extends Error {
  readonly code: ExportProblem;
  readonly detail: string;

  constructor(code: ExportProblem, detail: string) {
    super(`${code}: ${detail}`);
    this.name = 'ExportError';
    this.code = code;
    this.detail = detail;
  }
}

/** One export, read whole. */
export interface Snapshot {
  manifest: Manifest;
  fleet: Fleet;
  repos: Map<string, RepoDoc>;
}

// The two kinds of file of an export.
const FLEET_PATH = 'fleet.json';
// The folder of the documents of the repositories, and the usual name of an export inside a bigger folder (a data repository).
const REPOS_DIR = 'repos';
const EXPORT_DIR = 'export';
const REPO_PATH = /^repos\/([A-Za-z0-9_.-]{1,140})\.json$/;

/** The file at a path of the folder, by walking it: a folder chosen through an input has no other way. */
async function fileAt(root: DirHandleLike, path: string): Promise<FileHandleLike | null> {
  const parts = path.split('/');
  let dir = root;
  for (const [i, part] of parts.entries()) {
    let found: FileHandleLike | DirHandleLike | null = null;
    for await (const entry of dir.values()) {
      if (entry.name === part) {
        found = entry;
        break;
      }
    }
    if (found === null) return null;
    if (i === parts.length - 1) return found.kind === 'file' ? found : null;
    if (found.kind !== 'directory') return null;
    dir = found;
  }
  return null;
}

function hex(bytes: ArrayBuffer): string {
  return [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

function parse(bytes: ArrayBuffer, path: string): unknown {
  try {
    return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
  } catch {
    throw new ExportError('invalid', `${path}: not JSON`);
  }
}

/** The version of the contract a document names; `null`: it names none. */
function versionOf(doc: unknown): number | null {
  const schema = typeof doc === 'object' && doc !== null ? (doc as { schema?: unknown }).schema : null;
  const match = typeof schema === 'string' ? /^testing-export\/(\d+)$/.exec(schema) : null;
  return match ? Number(match[1]) : null;
}

/** The check of one document: a newer contract first, then the contract, then the form an export has. */
function checked(doc: unknown, path: string): unknown {
  const version = versionOf(doc);
  if (version !== null && version > SCHEMA_VERSION) throw new ExportError('too_new', `testing-export/${version}`);
  if (!validate(doc)) {
    const first = validate.errors?.[0];
    // Where and which rule: the message of the check could quote a value of the document.
    throw new ExportError('invalid', `${path}${first?.instancePath ?? ''} (${first?.keyword ?? 'contract'})`);
  }
  if ((doc as { form?: unknown }).form !== 'export') throw new ExportError('not_export', path);
  return doc;
}

/**
 * The folder of the export the person meant: the chosen one, or its subfolder `export`. The picker shows folders only, so
 * nobody sees the manifest while choosing; choosing one level too high is the ordinary mistake. One level and one name: a
 * search through a whole tree of repositories could take an export nobody meant.
 */
async function exportFolder(chosen: DirHandleLike): Promise<DirHandleLike | null> {
  if ((await fileAt(chosen, MANIFEST)) !== null) return chosen;
  for await (const entry of chosen.values()) {
    if (entry.kind === 'directory' && entry.name === EXPORT_DIR) {
      return (await fileAt(entry, MANIFEST)) !== null ? entry : null;
    }
  }
  return null;
}

/** Read one export whole, or refuse it. */
export async function readExport(chosen: DirHandleLike, maxTotal: number = MAX_TOTAL_BYTES): Promise<Snapshot> {
  const root = await exportFolder(chosen);
  // A folder cannot be asked for its parent: `repos` can only say where to go.
  if (root === null) throw new ExportError(chosen.name === REPOS_DIR ? 'repos_folder' : 'no_manifest', MANIFEST);
  const manifestFile = await fileAt(root, MANIFEST);
  if (manifestFile === null) throw new ExportError('no_manifest', MANIFEST);
  const manifestBlob = await manifestFile.getFile();
  if (manifestBlob.size > MAX_DOC_BYTES) throw new ExportError('too_large', MANIFEST);
  const manifest = checked(parse(await manifestBlob.arrayBuffer(), MANIFEST), MANIFEST) as Manifest;
  if (manifest.kind !== 'manifest') throw new ExportError('invalid', `${MANIFEST}: kind`);
  if (manifest.files.length > MAX_FILES) throw new ExportError('too_large', MANIFEST);

  let fleet: Fleet | null = null;
  let total = 0;
  const repos = new Map<string, RepoDoc>();
  for (const entry of manifest.files) {
    const path = entry.path;
    const repoKey = REPO_PATH.exec(path)?.[1];
    // The contract names these two kinds and no way out of the folder; a name too long for a key is refused here.
    if (path !== FLEET_PATH && repoKey === undefined) throw new ExportError('bad_path', path.slice(0, 200));
    const file = await fileAt(root, path);
    if (file === null) throw new ExportError('missing', path);
    const blob = await file.getFile();
    total += blob.size;
    if (blob.size > MAX_DOC_BYTES || total > maxTotal) throw new ExportError('too_large', path);
    if (blob.size !== entry.bytes) throw new ExportError('hash', path);
    const bytes = await blob.arrayBuffer();
    if (hex(await crypto.subtle.digest('SHA-256', bytes)) !== entry.sha256) throw new ExportError('hash', path);
    const doc = checked(parse(bytes, path), path) as { kind?: unknown };
    if (repoKey === undefined) {
      if (doc.kind !== 'fleet') throw new ExportError('invalid', `${path}: kind`);
      fleet = doc as Fleet;
    } else {
      if (doc.kind !== 'repo' || (doc as RepoDoc).repo.key !== repoKey) throw new ExportError('invalid', `${path}: kind or key`);
      repos.set(repoKey, doc as RepoDoc);
    }
  }
  if (fleet === null) throw new ExportError('missing', FLEET_PATH);
  return { manifest, fleet, repos };
}

/** What the folder looks like now, without reading a document: the sizes and times of the files the page reads. */
export async function signatureOf(chosen: DirHandleLike): Promise<string> {
  // The same folder the read takes: else a new export in a subfolder would never be noticed.
  const root = (await exportFolder(chosen)) ?? chosen;
  const lines: string[] = [];
  const add = async (handle: FileHandleLike, path: string): Promise<void> => {
    const file = await handle.getFile();
    lines.push(`${path} ${file.size} ${file.lastModified ?? 0}`);
  };
  for await (const entry of root.values()) {
    if (entry.kind === 'file' && (entry.name === MANIFEST || entry.name === FLEET_PATH)) await add(entry, entry.name);
    if (entry.kind === 'directory' && entry.name === REPOS_DIR) {
      for await (const inner of entry.values()) {
        if (inner.kind === 'file') await add(inner, `repos/${inner.name}`);
      }
    }
  }
  return lines.sort().join('\n');
}

export class FolderSource implements TestingSource {
  readonly capabilities: Capabilities = { session: false, about: false, findings: false, step: false };
  private snapshot: Snapshot;

  constructor(snapshot: Snapshot) {
    this.snapshot = snapshot;
  }

  /** Take a new export; the one held stays until this is called, so a page never sees a part of one. */
  replace(snapshot: Snapshot): void {
    this.snapshot = snapshot;
  }

  /** When the export was written, as the manifest says. */
  get generatedAt(): string {
    return this.snapshot.manifest.generated_at;
  }

  fleet(etag: string | null): Promise<FleetResult> {
    const { fleet } = this.snapshot;
    if (etag !== null && etag === fleet.digest) return Promise.resolve({ kind: 'not_modified' });
    return Promise.resolve({ kind: 'fresh', doc: fleet, etag: fleet.digest });
  }

  repo(key: string): Promise<RepoDoc> {
    const doc = this.snapshot.repos.get(key);
    return doc ? Promise.resolve(doc) : Promise.reject(new ExportError('missing', `repos/${key}.json`));
  }
}
