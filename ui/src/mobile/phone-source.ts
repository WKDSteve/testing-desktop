// The sources of testing mobil. Through a hub (Tailscale Serve or an SSH tunnel), `HubSource` of the page works as it
// is, over the bridge to the shell (`ipcFetch`); the phone only reads and starts nothing but a rerun, so it says it can
// step no sheet. Through a cloned vault, the export folder is read as the web variant reads one (`readExport`), over a
// folder handle of the shell.

import type { DirHandleLike, FileHandleLike } from 'appkit-web/folder';
import { ipcFetch, KEPT_AT, LinkError, type LinkFailure } from 'appkit-ui/mobile';
import { etagOf, failure } from 'appkit-ui/client';
import type { Capabilities, FleetResult } from '../lib/source';
import { HubSource } from '../lib/source';
import type { Fleet } from '../lib/contract';
import type { Shell } from './shell';

/** The hub, over the shell. */
export class PhoneSource extends HubSource {
  override readonly capabilities: Capabilities = { session: false, about: true, findings: true, step: false };
  /** When the fleet on the page is the copy the shell kept (seconds since 1970), else `null`. */
  keptAt: number | null = null;
  /** Why the last read of the fleet failed, as a word of the shell. */
  lastFailure: LinkFailure | null = null;

  constructor(shell: Shell) {
    super(ipcFetch(shell.bridge));
  }

  override async fleet(etag: string | null): Promise<FleetResult> {
    const headers: Record<string, string> = {};
    if (etag) headers['If-None-Match'] = `"${etag}"`;
    try {
      const response = await this.fetcher(this.route('fleet'), { headers });
      if (response.status === 304) return { kind: 'not_modified' };
      if (!response.ok) throw await failure(response);
      const kept = response.headers.get(KEPT_AT);
      this.keptAt = kept !== null && /^\d{1,12}$/.test(kept) ? Number(kept) : null;
      this.lastFailure = null;
      return { kind: 'fresh', doc: (await response.json()) as Fleet, etag: etagOf(response.headers.get('etag')) };
    } catch (error) {
      this.lastFailure = error instanceof LinkError ? error.kind : 'failed';
      throw error;
    }
  }
}

/** The longest name of an entry, and the most entries of a folder, that are read. */
const MAX_NAME = 255;

function file(shell: Shell, path: string, name: string, size: number): FileHandleLike {
  return {
    kind: 'file',
    name,
    getFile: async () => ({
      size,
      arrayBuffer: async () => new TextEncoder().encode(await shell.vaultRead(path)).buffer as ArrayBuffer,
    }),
  };
}

/** The folder the person chose for the cloned vault, or a folder below it, as the web variant's reader takes one. */
export function vaultFolder(shell: Shell, path = '', name = ''): DirHandleLike {
  return {
    kind: 'directory',
    name,
    values: async function* () {
      for (const e of await shell.vaultList(path)) {
        if (e.name.length === 0 || e.name.length > MAX_NAME || e.name.includes('/') || e.name === '.' || e.name === '..') continue;
        const below = path === '' ? e.name : `${path}/${e.name}`;
        yield e.dir ? vaultFolder(shell, below, e.name) : file(shell, below, e.name, e.size);
      }
    },
  };
}
