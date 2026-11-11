// The page of testing mobil, inside the Tauri shell (mobile/). It never reaches the network itself: every request goes
// over the bridge to the shell (appkit-ui/mobile `ipcFetch`), which holds the way, the keys and the session. The policy
// of the shell's WebView allows `connect-src ipc:` only.
//
// The tokens and base rules of appkit-ui first, the palette of this app between them, as on the hub.
import 'appkit-ui/tokens.css';
import './palette.css';
import 'appkit-ui/base.css';
import { mount } from 'svelte';
import MobilePage from './mobile/MobilePage.svelte';
import { Shell, tauriInvoke } from './mobile/shell';

const target = document.getElementById('app');
if (target === null) throw new Error('#app is missing');

const invoke = tauriInvoke();
if (invoke === null) {
  target.textContent = 'testing mobil runs inside its app only.';
} else {
  mount(MobilePage, { target, props: { shell: new Shell(invoke), languages: navigator.languages } });
}
