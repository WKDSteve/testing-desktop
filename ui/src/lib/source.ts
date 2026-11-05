// Where the page gets its documents from. The views know nothing of the transport: the hub answers over HTTP here,
// a folder of an export answers in the web variant (testing-web), both through `TestingSource`. Only the documents
// of `testing-export/1` cross it, as the engine wrote them.

import type { Fleet, FindingsDoc, RepoDoc } from './contract';
import { ApiError, etagOf, failure, HubClient, textOf } from 'appkit-ui/client';

export { ApiError };

/** What a source can do besides reading; the views leave out what it cannot. */
export interface Capabilities {
  /** It can pair and say whether its engine is ready (the hub); a folder has neither. */
  session: boolean;
  /** It says which engine answers (`about`). */
  about: boolean;
}

/** A read of the fleet: a new document, or the one the page holds is still the current one. */
export type FleetResult = { kind: 'fresh'; doc: Fleet; etag: string | null } | { kind: 'not_modified' };

/** The build and the engine that answers, and where the hub reads from (as words, never a path). */
export interface About {
  app: string;
  version: string;
  engine: { version: string | null; git: string | null; nvim: string | null } | null;
  engineProblem: string | null;
  reads: { stateDir: string | null; areas: string | null };
  /** Whether the hub asks GitHub, and why not: `file`, `gh`, `no_token`, `engine_too_old`, `no_engine`, `off`. */
  ci: string | null;
}

export interface TestingSource {
  readonly capabilities: Capabilities;
  /** The fleet; with the tag of the one held, `not_modified` when it is still the current one. */
  fleet(etag: string | null): Promise<FleetResult>;
  /** One repository with its red cases, by its key of the fleet. */
  repo(key: string, signal?: AbortSignal): Promise<RepoDoc>;
}

/** Where the routes of this app live (`IDENTITY.route` of testing-hub). */
export const API = '/api/testing';

/** The hub of testing-desktop: the transport (token, refusal, pairing, hello) is the kit's. */
export class HubSource extends HubClient<Fleet> implements TestingSource {
  readonly capabilities: Capabilities = { session: true, about: true };

  constructor(fetchFn: typeof fetch, base: string = '') {
    super(fetchFn, API, base);
  }

  /** The `fetch` the event stream is read with (the kit's `StreamSession`). */
  get fetcher(): typeof fetch {
    return this.fetchFn;
  }

  async fleet(etag: string | null): Promise<FleetResult> {
    const headers: Record<string, string> = {};
    if (etag) headers['If-None-Match'] = `"${etag}"`;
    const response = await this.request(this.route('fleet'), { headers });
    if (response.status === 304) return { kind: 'not_modified' };
    if (!response.ok) throw await failure(response);
    return { kind: 'fresh', doc: (await response.json()) as Fleet, etag: etagOf(response.headers.get('etag')) };
  }

  repo(key: string, signal?: AbortSignal): Promise<RepoDoc> {
    const path = `${this.route('repo')}?key=${encodeURIComponent(key)}`;
    return this.json<RepoDoc>(path, signal ? { signal } : undefined);
  }

  /** The findings of the last run of one repository (the hub only; an export folder holds none). */
  finding(key: string, signal?: AbortSignal): Promise<FindingsDoc> {
    const path = `${this.route('finding')}?key=${encodeURIComponent(key)}`;
    return this.json<FindingsDoc>(path, signal ? { signal } : undefined);
  }

  /** What this build is and which engine answers: only the words the page shows are taken. */
  async about(): Promise<About> {
    const doc = await this.json<Record<string, unknown>>(this.route('about'));
    const engine = doc['engine'];
    const reads = doc['reads'];
    const field = (obj: unknown, key: string): string | null =>
      typeof obj === 'object' && obj !== null && !Array.isArray(obj) ? textOf((obj as Record<string, unknown>)[key]) : null;
    return {
      app: textOf(doc['app']) ?? 'testing-desktop',
      version: textOf(doc['version']) ?? '',
      engine:
        typeof engine === 'object' && engine !== null && !Array.isArray(engine)
          ? { version: field(engine, 'version'), git: field(engine, 'git'), nvim: field(engine, 'nvim') }
          : null,
      engineProblem: textOf(doc['engine_problem']),
      reads: { stateDir: field(reads, 'state_dir'), areas: field(reads, 'areas') },
      ci: textOf(doc['ci']),
    };
  }
}
