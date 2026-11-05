import { readdirSync, readFileSync, statSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import type { DirHandleLike, FileHandleLike } from 'appkit-web/folder';
import { describe, expect, it } from 'vitest';
import { ExportError, FolderSource, MAX_DOC_BYTES, MAX_TOTAL_BYTES, readExport, signatureOf, type Snapshot } from '../src/web/folder-source';

const GOLDEN = join(__dirname, '..', '..', 'crates', 'testing-hub', 'tests', 'fixtures', 'golden-export');
const ALPHA = 'alpha.nvim-356bf237c7fc';

type Tree = Map<string, Uint8Array>;

function golden(): Tree {
  const tree: Tree = new Map();
  const walk = (dir: string, prefix: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path, `${prefix}${name}/`);
      else tree.set(`${prefix}${name}`, new Uint8Array(readFileSync(path)));
    }
  };
  walk(GOLDEN, '');
  return tree;
}

/** A folder over a map of paths, as the browser hands one out; `time` is what `lastModified` says. */
function folder(tree: Tree, time = 1, rootName = 'export'): DirHandleLike {
  const dir = (prefix: string, name: string): DirHandleLike => ({
    kind: 'directory',
    name,
    async *values() {
      const seen = new Set<string>();
      for (const [path, bytes] of tree) {
        if (!path.startsWith(prefix)) continue;
        const rest = path.slice(prefix.length);
        const head = rest.split('/')[0] as string;
        if (seen.has(head)) continue;
        seen.add(head);
        if (rest.includes('/')) {
          yield dir(`${prefix}${head}/`, head);
        } else {
          const file: FileHandleLike = {
            kind: 'file',
            name: head,
            getFile: () =>
              Promise.resolve({
                size: bytes.length,
                lastModified: time,
                arrayBuffer: () => Promise.resolve(bytes.slice().buffer),
              }),
          };
          yield file;
        }
      }
    },
  });
  return dir('', rootName);
}

const text = (bytes: Uint8Array | undefined) => new TextDecoder().decode(bytes);
const encode = (s: string) => new TextEncoder().encode(s);

/** The golden export with one document changed, and the manifest made right for it: a good sum for a bad document. */
function withDoc(tree: Tree, path: string, change: (doc: Record<string, unknown>) => void): Tree {
  const doc = JSON.parse(text(tree.get(path))) as Record<string, unknown>;
  change(doc);
  const bytes = encode(JSON.stringify(doc));
  const next = new Map(tree).set(path, bytes);
  const manifest = JSON.parse(text(tree.get('testing-export.json'))) as { files: { path: string; bytes: number; sha256: string }[] };
  for (const f of manifest.files) {
    if (f.path === path) {
      f.bytes = bytes.length;
      f.sha256 = createHash('sha256').update(bytes).digest('hex');
    }
  }
  return next.set('testing-export.json', encode(JSON.stringify(manifest)));
}

async function refused(tree: Tree): Promise<ExportError> {
  try {
    await readExport(folder(tree));
  } catch (error) {
    expect(error).toBeInstanceOf(ExportError);
    return error as ExportError;
  }
  throw new Error('the export was taken');
}

describe('reading an export', () => {
  it('takes the golden export whole: the fleet, every repository, the manifest', async () => {
    const snapshot = await readExport(folder(golden()));
    expect(snapshot.manifest.kind).toBe('manifest');
    expect(snapshot.fleet.kind).toBe('fleet');
    expect([...snapshot.repos.keys()].sort()).toEqual([ALPHA, 'beta.nvim-ac6d3a4ece7f', 'gamma.nvim-bed2b5150df9']);
    for (const row of snapshot.fleet.repos) expect(snapshot.repos.has(row.key), row.key).toBe(true);
  });

  it('refuses a folder without a manifest, and a manifest that is no JSON', async () => {
    const none = golden();
    none.delete('testing-export.json');
    expect((await refused(none)).code).toBe('no_manifest');
    expect((await refused(new Map(golden()).set('testing-export.json', encode('{')))).code).toBe('invalid');
  });

  it('finds the export in the subfolder export, the mistake of choosing one level too high', async () => {
    const nested: Tree = new Map([...golden()].map(([path, bytes]) => [`export/${path}`, bytes]));
    nested.set('README.md', encode('the data repository'));
    const snapshot = await readExport(folder(nested, 1, 'testing-results'));
    expect(snapshot.repos.has(ALPHA)).toBe(true);
  });

  it('takes the chosen folder first, and looks no further than one level and that one name', async () => {
    const own = golden();
    const both: Tree = new Map(own);
    for (const [path, bytes] of own) both.set(`export/${path}`, path === 'fleet.json' ? encode('{') : bytes);
    // The broken inner one is not read: the folder chosen is an export itself.
    expect((await readExport(folder(both))).repos.has(ALPHA)).toBe(true);

    const other: Tree = new Map([...own].map(([path, bytes]) => [`out/${path}`, bytes]));
    await expect(readExport(folder(other, 1, 'data'))).rejects.toMatchObject({ code: 'no_manifest' });
    const deep: Tree = new Map([...own].map(([path, bytes]) => [`a/export/${path}`, bytes]));
    await expect(readExport(folder(deep, 1, 'data'))).rejects.toMatchObject({ code: 'no_manifest' });
    const emptyExport: Tree = new Map([['export/notes.txt', encode('x')]]);
    await expect(readExport(folder(emptyExport, 1, 'data'))).rejects.toMatchObject({ code: 'no_manifest' });
  });

  it('says where to go when the folder repos of an export was chosen', async () => {
    const inner: Tree = new Map([[`${ALPHA}.json`, encode('{}')]]);
    await expect(readExport(folder(inner, 1, 'repos'))).rejects.toMatchObject({ code: 'repos_folder' });
  });

  it('refuses a file that does not match its sum, even of the same size', async () => {
    const tree = golden();
    const bytes = (tree.get('fleet.json') as Uint8Array).slice();
    bytes[10] = bytes[10] === 0x20 ? 0x21 : 0x20;
    const error = await refused(new Map(tree).set('fleet.json', bytes));
    expect(error.code).toBe('hash');
    expect(error.detail).toBe('fleet.json');
  });

  it('refuses a file of another size before it reads it', async () => {
    expect((await refused(new Map(golden()).set('fleet.json', encode('{}')))).code).toBe('hash');
  });

  it('refuses half an export: a file the manifest names is missing', async () => {
    const tree = golden();
    tree.delete(`repos/${ALPHA}.json`);
    const error = await refused(tree);
    expect(error.code).toBe('missing');
    expect(error.detail).toBe(`repos/${ALPHA}.json`);
  });

  it('says that the page is too old for a higher version of the contract', async () => {
    const tree = golden();
    const manifest = JSON.parse(text(tree.get('testing-export.json'))) as Record<string, unknown>;
    manifest['schema'] = 'testing-export/2';
    const error = await refused(new Map(tree).set('testing-export.json', encode(JSON.stringify(manifest))));
    expect(error.code).toBe('too_new');
    expect(error.detail).toBe('testing-export/2');
    // The same for a document behind a manifest it knows.
    const inner = await refused(withDoc(golden(), 'fleet.json', (d) => (d['schema'] = 'testing-export/9')));
    expect(inner.code).toBe('too_new');
  });

  it('refuses a document the contract does not know, without quoting its text', async () => {
    const error = await refused(withDoc(golden(), 'fleet.json', (d) => (d['secret'] = 'hunter2')));
    expect(error.code).toBe('invalid');
    expect(error.message).not.toContain('hunter2');
  });

  it('refuses the local form: it holds absolute paths', async () => {
    const tree = golden();
    const manifest = JSON.parse(text(tree.get('testing-export.json'))) as Record<string, unknown>;
    manifest['form'] = 'local';
    const error = await refused(new Map(tree).set('testing-export.json', encode(JSON.stringify(manifest))));
    expect(['not_export', 'invalid']).toContain(error.code);
  });

  it('refuses a path that climbs out of the folder, and never reads it', async () => {
    for (const path of ['../secret.json', 'repos/../../secret.json', '/etc/passwd', 'C:\\secret.json', 'repos/./x.json']) {
      const tree = golden();
      const manifest = JSON.parse(text(tree.get('testing-export.json'))) as { files: { path: string; bytes: number; sha256: string }[] };
      manifest.files.unshift({ path, bytes: 2, sha256: '0'.repeat(64) });
      const error = await refused(new Map(tree).set('testing-export.json', encode(JSON.stringify(manifest))));
      expect(['bad_path', 'invalid'], path).toContain(error.code);
    }
  });

  it('refuses a repository under the key of another', async () => {
    const error = await refused(
      withDoc(golden(), `repos/${ALPHA}.json`, (d) => {
        (d['repo'] as { key: string }).key = 'beta.nvim-ac6d3a4ece7f';
      }),
    );
    expect(error.code).toBe('invalid');
  });

  it('refuses a file larger than a document can be, without reading it', async () => {
    const big = new Uint8Array(8 * 1024 * 1024 + 1);
    expect((await refused(new Map(golden()).set('testing-export.json', big))).code).toBe('too_large');
    expect((await refused(new Map(golden()).set('fleet.json', big))).code).toBe('too_large');
  });

  it('refuses an export too large to hold, though each file is small enough', async () => {
    const whole = golden();
    const sizes = [...whole.entries()].filter(([path]) => path !== 'testing-export.json').map(([, bytes]) => bytes.length);
    const documents = sizes.reduce((a, b) => a + b, 0);
    await expect(readExport(folder(whole), documents)).resolves.toBeDefined();
    await expect(readExport(folder(whole), documents - 1)).rejects.toMatchObject({ code: 'too_large' });
    expect(MAX_TOTAL_BYTES).toBeGreaterThan(MAX_DOC_BYTES);
  });

  it('refuses a name too long to be a key, which the contract lets through', async () => {
    const tree = golden();
    const manifest = JSON.parse(text(tree.get('testing-export.json'))) as { files: { path: string; bytes: number; sha256: string }[] };
    manifest.files.push({ path: `repos/${'a'.repeat(141)}.json`, bytes: 2, sha256: '0'.repeat(64) });
    const error = await refused(new Map(tree).set('testing-export.json', encode(JSON.stringify(manifest))));
    expect(error.code).toBe('bad_path');
  });

  it('refuses a kind of file the contract does not know', async () => {
    const tree = golden();
    const manifest = JSON.parse(text(tree.get('testing-export.json'))) as { files: { path: string; bytes: number; sha256: string }[] };
    manifest.files.push({ path: 'runs/alpha/1.json', bytes: 2, sha256: '0'.repeat(64) });
    const error = await refused(new Map(tree).set('testing-export.json', encode(JSON.stringify(manifest))));
    expect(error.code).toBe('invalid');
  });
});

describe('the source', () => {
  const snapshot = (): Promise<Snapshot> => readExport(folder(golden()));

  it('answers the fleet with its digest as the tag, and a repository by its key', async () => {
    const source = new FolderSource(await snapshot());
    const first = await source.fleet(null);
    expect(first.kind).toBe('fresh');
    if (first.kind !== 'fresh') throw new Error('fresh');
    expect(first.etag).toBe(first.doc.digest);
    expect(await source.fleet(first.etag)).toEqual({ kind: 'not_modified' });
    expect((await source.repo(ALPHA)).kind).toBe('repo');
    await expect(source.repo('nope.nvim-000000000000')).rejects.toBeInstanceOf(ExportError);
  });

  it('says that it has no session, no engine and no findings', async () => {
    expect(new FolderSource(await snapshot()).capabilities).toEqual({ session: false, about: false, findings: false });
  });
});

describe('the signature of the folder', () => {
  it('follows the files the page reads, and nothing else in the folder', async () => {
    const tree = golden();
    const before = await signatureOf(folder(tree));
    expect(await signatureOf(folder(tree))).toBe(before);
    expect(await signatureOf(folder(tree, 2))).not.toBe(before);
    expect(await signatureOf(folder(new Map(tree).set(`repos/${ALPHA}.json`, encode('x'))))).not.toBe(before);
    // The daten repository is a git repository: what git does there is not a new export.
    expect(await signatureOf(folder(new Map(tree).set('.git/HEAD', encode('x'))))).toBe(before);
    expect(await signatureOf(folder(new Map(tree).set('README.md', encode('x'))))).toBe(before);
    // An export in the subfolder export: the signature looks where the read looks.
    const nested: Tree = new Map([...tree].map(([path, bytes]) => [`export/${path}`, bytes]));
    const outer = await signatureOf(folder(nested, 1, 'data'));
    expect(outer).toBe(before);
    expect(await signatureOf(folder(nested, 2, 'data'))).not.toBe(outer);
  });
});
