// The page of the web variant: one HTML file opened from disk, the results a folder the person chose. No server and no
// pairing; nothing here or below reaches the network, and the policy of the page forbids it.
//
// The tokens and base rules of appkit-ui first, the palette of this app between them, as on the hub.
import 'appkit-ui/tokens.css';
import './palette.css';
import 'appkit-ui/base.css';
import { idbKeyValue, RecentFolders, type PickerWindow } from 'appkit-web/folder';
import { mount } from 'svelte';
import logo from '../../assets/icons/testing-web/testing-web-small.svg?url';
import { Entry } from './web/entry';
import { HANDLES_DB } from './web/folder';
import { readExport } from './web/folder-source';
import WebPage from './web/WebPage.svelte';

declare const __WEB_BUILD__: string | null;

const target = document.getElementById('app');
if (target === null) throw new Error('#app is missing');

// The shell may name no file, so the icon of the tab comes in here, as a data: URL.
document.head.append(Object.assign(document.createElement('link'), { rel: 'icon', href: logo }));

const entry = new Entry({
  win: window as unknown as PickerWindow,
  recent: new RecentFolders(idbKeyValue(window.indexedDB, HANDLES_DB)),
  read: (handle) => readExport(handle),
  now: () => Date.now(),
});

mount(WebPage, { target, props: { entry, languages: navigator.languages, build: __WEB_BUILD__ } });
