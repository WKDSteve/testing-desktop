// Packs the page of the web variant into one file.
//
//   pnpm web:build            (= vite build --mode web, then this)
//   node web/pack.mjs [--dist=<folder of vite build --mode web>] [--out=<folder>]    (from ui/)
//
// In:  dist-web/index.web.html with its one script and its one style.
// Out: <out>/testing.html and <out>/SHA256SUMS. Everything is in the file, with a Content-Security-Policy as a meta tag
//      that forbids every request. The page is opened from disk (file://) and needs no server.
//
// The packing and the check are the kit's (appkit-web/pack). This page runs no engine, so it carries no part and takes
// the strict policy (no WASM, no worker). The build refuses a file whose shell loads anything; what the compiled script
// does at run time is checked by the browser test (e2e-web/), which opens the file and counts the requests.

import { execFileSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { assemble as assemblePage, CSP_STRICT, readDist as readKitDist, verifyShell, writePage } from 'appkit-web/pack';

const ui = join(dirname(fileURLToPath(import.meta.url)), '..');

export { verifyShell };
export const CSP = CSP_STRICT;

/** The single file. */
export function assemble({ css, js, build }) {
  return assemblePage({ css, js, csp: CSP, title: 'testing', comment: `testing-desktop web variant, build ${build}` });
}

/** The script and the style `vite build --mode web` made, found through its index. */
export function readDist(dist) {
  return readKitDist(dist, 'index.web.html');
}

function commit() {
  try {
    return execFileSync('git', ['-C', ui, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim();
  } catch {
    return 'unknown';
  }
}

function main() {
  const arg = (name, fallback) => {
    const given = process.argv.find((a) => a.startsWith(`--${name}=`));
    return resolve(given ? given.slice(name.length + 3) : fallback);
  };
  const dist = arg('dist', join(ui, 'dist-web'));
  const out = arg('out', dist);
  const { js, css } = readDist(dist);
  const html = assemble({ css, js, build: commit() });
  const problems = verifyShell(html);
  if (problems.length) {
    for (const p of problems) console.error('pack: ' + p);
    process.exit(1);
  }
  const sha = writePage(out, 'testing.html', html);
  console.log(`${sha}  ${join(out, 'testing.html')} (${html.length} bytes: script ${js.length}, style ${css.length})`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
