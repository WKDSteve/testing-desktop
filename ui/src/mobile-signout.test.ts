// The phone page has no tab session to sign out of: it is paired once with a key of its own, so its way out is
// "Unpair this phone" in the settings, behind a question. It must stay reachable and must really forget the pairing
// (the keys, the kept copy, the folder and the push on the phone: `phone_forget` of the shell). This test reads the
// component; a browser test cannot, because the page needs the shell of the phone.

import { describe, expect, it } from 'vitest';

const COMPONENTS = import.meta.glob('./mobile/*.svelte', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const page = COMPONENTS['./mobile/MobilePage.svelte'] ?? '';

describe('the phone page can be unpaired', () => {
  it('reads the page', () => {
    expect(page.length).toBeGreaterThan(1000);
  });

  it('shows the button in the settings of a paired phone and asks before it acts', () => {
    const settings = page.slice(page.indexOf('<section class="settings"'), page.indexOf('</section>', page.indexOf('<section class="settings"')));
    expect(settings).toMatch(/\{#if phone\.paired && !phone\.vault\}[^]*askUnpair = true[^]*mobile\.unpair'/);
    expect(page).toMatch(/\{#if askUnpair\}[^]*onclick=\{unpair\}[^]*mobile\.unpair\.yes/);
  });

  it('forgets the pairing through the shell, and shows the pairing page again', () => {
    expect(page).toMatch(/async function unpair\(\)[^]*await open\(await shell\.forget\(\)\)/);
  });
});
