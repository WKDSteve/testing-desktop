// What the page of testing mobil reads through a hub: `Reader`'s fleet and repository, and what the phone adds, the age
// of a kept copy and the word of the last failure (MOB-08, MUI-05). It reads when the app is opened or comes back to
// the front and when the person asks: no polling and no event stream on a phone (MUI-08).

import type { LinkFailure } from 'appkit-ui/mobile';
import { Reader } from '../lib/reader.svelte';
import type { PhoneSource } from './phone-source';

export class MobileReader extends Reader {
  /** The fleet on the page is the kept copy from this time (seconds since 1970); `null`: from the hub just now. */
  keptAt = $state<number | null>(null);
  /** Why the last read failed, as a word of the shell; `null`: it did not. */
  linkFailure = $state<LinkFailure | null>(null);

  constructor(private readonly phone: PhoneSource) {
    super(phone);
  }

  start(): Promise<void> {
    return this.refresh();
  }

  stop(): void {
    this.stopped = true;
    this.repoAbort?.abort();
  }

  override async refresh(): Promise<void> {
    try {
      await super.refresh();
    } finally {
      this.keptAt = this.phone.keptAt;
      this.linkFailure = this.phone.lastFailure;
    }
  }
}
