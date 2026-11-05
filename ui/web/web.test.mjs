// The packing of the web page, the check it runs, and what its sources may not do. Node's file system and the built
// pieces only: the page itself is tested by opening it (e2e-web/).

import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { GENERATED, generate } from './gen-validate.mjs';
import { assemble, CSP, verifyShell } from './pack.mjs';
import { DICTIONARIES } from '../src/lib/i18n';
import validate from '../src/web/validate.generated.js';

const here = dirname(fileURLToPath(import.meta.url));
const WEB_SRC = join(here, '..', 'src', 'web');
const GOLDEN = join(here, '..', '..', 'crates', 'testing-hub', 'tests', 'fixtures');

/** The sources of the page's own: what the tests of this folder and the generated check are not. */
function sources() {
  return readdirSync(WEB_SRC)
    .filter((n) => /\.(ts|svelte)$/.test(n) && !n.includes('.test.') && !n.includes('.generated.'))
    .map((name) => ({ name, text: readFileSync(join(WEB_SRC, name), 'utf8') }));
}

/** A source without its comments, which may name what the code does not do. */
const code = (text) => text.replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:])\/\/.*$/gm, '$1');

describe('the packed page', () => {
  const html = assemble({ css: 'body{color:red}', js: 'console.log(1)', build: 'test' });

  it('holds its script and style inline and passes the check of the shell', () => {
    expect(verifyShell(html)).toEqual([]);
    expect(html).toContain('console.log(1)');
    expect(html).toContain('<title>testing</title>');
  });

  it('forbids every request, and neither WASM nor a worker is allowed', () => {
    for (const rule of ["default-src 'none'", "connect-src 'none'", "base-uri 'none'", "form-action 'none'"]) {
      expect(CSP).toContain(rule);
    }
    expect(CSP).not.toContain('wasm-unsafe-eval');
    expect(CSP).not.toContain('unsafe-eval');
    expect(CSP).not.toContain('worker-src');
    expect(html).toContain(CSP.replaceAll("'", '&#39;').length > 0 ? 'Content-Security-Policy' : '');
  });

  it('is refused when its shell names a file', () => {
    const bad = html.replace('</head>', '<script src="https://example.com/x.js"></script></head>');
    expect(verifyShell(bad).length).toBeGreaterThan(0);
  });
});

describe('the check of the documents', () => {
  it('is what the generator writes from the schema: a new schema needs `pnpm web:gen`', () => {
    expect(readFileSync(GENERATED, 'utf8').replaceAll('\r\n', '\n')).toBe(generate().replaceAll('\r\n', '\n'));
  });

  it('needs nothing at run time: no require, no eval', () => {
    const text = readFileSync(GENERATED, 'utf8');
    expect(text).not.toMatch(/\brequire\(/);
    expect(text).not.toMatch(/\bnew Function\b|\beval\(/);
  });

  it('takes every golden document', () => {
    const docs = [
      'golden/fleet.json',
      'golden/repo.json',
      'golden/hello.json',
      'golden/findings.json',
      'golden-export/testing-export.json',
      'golden-export/fleet.json',
      'golden-export/repos/alpha.nvim-356bf237c7fc.json',
      'golden-export/repos/beta.nvim-ac6d3a4ece7f.json',
      'golden-export/repos/gamma.nvim-bed2b5150df9.json',
    ];
    for (const doc of docs) {
      const ok = validate(JSON.parse(readFileSync(join(GOLDEN, doc), 'utf8')));
      expect(ok, `${doc}: ${JSON.stringify(validate.errors)}`).toBe(true);
    }
  });

  it('refuses what breaks the contract', () => {
    const fleet = JSON.parse(readFileSync(join(GOLDEN, 'golden-export/fleet.json'), 'utf8'));
    expect(validate({ ...fleet, extra: 1 })).toBe(false);
    expect(validate({ ...fleet, schema: 'testing-export/2' })).toBe(false);
    expect(validate({ ...fleet, repos: 'x' })).toBe(false);
    const { digest: _digest, ...noDigest } = fleet;
    expect(validate(noDigest)).toBe(false);
    expect(validate(null)).toBe(false);
    expect(validate([])).toBe(false);
  });
});

describe('the words of the way in', () => {
  it('has a word for every reason a folder is refused', () => {
    const entry = readFileSync(join(WEB_SRC, 'entry.ts'), 'utf8');
    const source = readFileSync(join(WEB_SRC, 'folder-source.ts'), 'utf8');
    const words = (text, name) => [...new RegExp(`${name}[^;]*;`).exec(text)[0].matchAll(/'([a-z_]+)'/g)].map((m) => m[1]);
    const codes = [...words(entry, "code: "), ...words(source, 'type ExportProblem =')];
    expect(codes).toEqual(expect.arrayContaining(['cancelled', 'gone', 'no_manifest', 'invalid']));
    for (const code of codes) {
      for (const lang of ['en', 'de']) expect(DICTIONARIES[lang][`web.notice.${code}`], `${lang}: ${code}`).toBeTruthy();
    }
  });
});

describe('the sources of the page', () => {
  it('have sources to look at', () => {
    expect(sources().map((s) => s.name)).toEqual(expect.arrayContaining(['folder-source.ts', 'entry.ts', 'folder.ts', 'WebPage.svelte']));
  });

  it('reach no network: no fetch, XMLHttpRequest, WebSocket or EventSource', () => {
    for (const { name, text } of sources()) {
      expect(code(text), name).not.toMatch(/\bfetch\s*\(|XMLHttpRequest|WebSocket|EventSource|sendBeacon/);
    }
  });

  it('open only the database of this page', () => {
    const folder = readFileSync(join(WEB_SRC, 'folder.ts'), 'utf8');
    expect(folder).toMatch(/HANDLES_DB = 'testing-web'/);
    const main = readFileSync(join(here, '..', 'src', 'main.web.ts'), 'utf8');
    expect(main).toMatch(/idbKeyValue\(window\.indexedDB, HANDLES_DB\)/);
    for (const { name, text } of sources()) {
      expect(code(text), name).not.toMatch(/indexedDB\.open|tasks-web|rules-web/);
    }
  });

  it('keep nothing of an export in the browser: no localStorage, sessionStorage or cache', () => {
    for (const { name, text } of sources()) {
      expect(code(text), name).not.toMatch(/localStorage|sessionStorage|caches\./);
    }
  });

  it('do not take the hub: no session, no stream', () => {
    const main = readFileSync(join(here, '..', 'src', 'main.web.ts'), 'utf8');
    for (const text of [main, ...sources().map((s) => s.text)]) {
      expect(code(text)).not.toMatch(/HubSource|StreamSession|appkit-ui\/session|app\.svelte/);
    }
  });
});
