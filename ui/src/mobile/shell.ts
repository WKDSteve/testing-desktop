// The bridge to the shell of testing mobil (mobile/src/lib.rs). The page reaches only these commands; the shell holds
// the way to the hub, the device keys, the session and the kept copy (appkit's mobile basis). Tauri's own `invoke` is
// taken from the page's global, so the page needs no package of Tauri.

import type { Bridge, BridgeRequest, BridgeResponse, LinkFailure, Way } from 'appkit-ui/mobile';

type Invoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

/** Tauri's `invoke`, or `null` outside the app (a test, the browser). */
export function tauriInvoke(win: unknown = globalThis): Invoke | null {
  const internals = (win as { __TAURI_INTERNALS__?: { invoke?: Invoke } }).__TAURI_INTERNALS__;
  return typeof internals?.invoke === 'function' ? internals.invoke.bind(internals) : null;
}

/** What the shell knows of the pairing; never a key, a token or the code. */
export interface PhoneState {
  paired: boolean;
  way: Way | null;
  hub: string | null;
  name: string | null;
  /** This phone can confirm a trigger (a key behind its lock). */
  confirm: boolean;
  vault: boolean;
}

export interface PushStatus {
  distributor: boolean;
  notices: boolean;
  subscribed: boolean;
}

/** A refusal of the shell, as a word; anything else is `failed`. */
export class ShellError extends Error {
  constructor(readonly kind: string) {
    super(kind);
  }
}

function kindOf(error: unknown): string {
  const kind = typeof error === 'string' ? error : (error as { kind?: unknown } | null)?.kind;
  return typeof kind === 'string' && /^[a-z_]{1,32}$/.test(kind) ? kind : 'failed';
}

function isWay(v: unknown): v is Way {
  return v === 'tailnet' || v === 'tunnel' || v === 'vault';
}

function phoneState(v: unknown): PhoneState {
  const o = (typeof v === 'object' && v !== null ? v : {}) as Record<string, unknown>;
  const text = (x: unknown): string | null => (typeof x === 'string' && x.length <= 300 ? x : null);
  return {
    paired: o['paired'] === true,
    way: isWay(o['way']) ? o['way'] : null,
    hub: text(o['hub']),
    name: text(o['name']),
    confirm: o['confirm'] === true,
    vault: o['vault'] === true,
  };
}

function pushStatus(v: unknown): PushStatus {
  const o = (typeof v === 'object' && v !== null ? v : {}) as Record<string, unknown>;
  return { distributor: o['distributor'] === true, notices: o['notices'] === true, subscribed: o['subscribed'] === true };
}

/** The commands of the shell, typed and checked. */
export class Shell {
  constructor(private readonly invoke: Invoke) {}

  private async call(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
    try {
      return await this.invoke(cmd, args);
    } catch (error) {
      throw new ShellError(kindOf(error));
    }
  }

  async state(): Promise<PhoneState> {
    return phoneState(await this.call('phone_state'));
  }

  /** The text of a QR code; `null` when the person went back. */
  async scan(): Promise<string | null> {
    const text = await this.call('phone_scan');
    return typeof text === 'string' && text.length <= 512 ? text : null;
  }

  async pair(link: string, name: string): Promise<PhoneState> {
    return phoneState(await this.call('phone_pair', { request: { link, name } }));
  }

  async forget(): Promise<PhoneState> {
    return phoneState(await this.call('phone_forget'));
  }

  /** The bridge `appkit-ui/mobile` `ipcFetch` turns into a `fetch`. */
  readonly bridge: Bridge = async (request: BridgeRequest): Promise<BridgeResponse> => {
    try {
      return (await this.invoke('link_fetch', { request })) as BridgeResponse;
    } catch (error) {
      throw { kind: kindOf(error) as LinkFailure };
    }
  };

  /** Start something, confirmed on the phone: the status and the body of the hub's answer. */
  async trigger(path: string, body: string): Promise<{ status: number; body: string }> {
    const r = (await this.call('link_trigger', { request: { path, body } })) as { status?: unknown; body?: unknown };
    return { status: typeof r.status === 'number' ? r.status : 0, body: typeof r.body === 'string' ? r.body : '' };
  }

  async vaultPick(): Promise<PhoneState> {
    return phoneState(await this.call('vault_pick'));
  }

  async vaultOff(): Promise<PhoneState> {
    return phoneState(await this.call('vault_off'));
  }

  async vaultList(path: string): Promise<{ name: string; dir: boolean; size: number }[]> {
    const list = await this.call('vault_list', { path });
    if (!Array.isArray(list)) return [];
    return list.filter(
      (e): e is { name: string; dir: boolean; size: number } =>
        typeof e === 'object' && e !== null && typeof e.name === 'string' && typeof e.dir === 'boolean' && typeof e.size === 'number',
    );
  }

  async vaultRead(path: string): Promise<string> {
    const text = await this.call('vault_read', { path });
    if (typeof text !== 'string') throw new ShellError('failed');
    return text;
  }

  async pushStatus(): Promise<PushStatus> {
    return pushStatus(await this.call('push_status'));
  }

  async pushOn(): Promise<PushStatus> {
    return pushStatus(await this.call('push_on'));
  }

  async pushOff(): Promise<PushStatus> {
    return pushStatus(await this.call('push_off'));
  }
}
