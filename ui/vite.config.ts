import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

const test = {
  environment: 'node' as const,
  // e2e/*.test.ts: the cleanup of the browser tests, checked without a hub (the browser tests are e2e/*.spec.ts).
  // web/: the packing, the checks and the sources of the web page (with Node's file system, so not under src/).
  include: ['src/**/*.test.ts', 'e2e/**/*.test.ts', 'web/**/*.test.mjs', 'web/**/*.test.ts'],
};

// `vite build --mode web` builds the page of the web variant: one module with everything in it, out of `dist-web/`.
// `web/pack.mjs` turns that into the single `testing.html`. Nothing in it may fetch: no preload, no split chunk, no file
// next to the page, so an icon is a data: URL.
const web = defineConfig({
  plugins: [svelte()],
  base: './',
  define: {
    __WEB_BUILD__: JSON.stringify(process.env['WEB_BUILD'] ?? null),
  },
  build: {
    outDir: 'dist-web',
    target: 'es2022',
    assetsInlineLimit: (file) => file.endsWith('.svg'),
    modulePreload: false,
    cssCodeSplit: false,
    sourcemap: false,
    chunkSizeWarningLimit: 700,
    rollupOptions: {
      input: 'index.web.html',
      output: { codeSplitting: false },
    },
  },
  test,
});

// The page is served by the hub on a fixed origin. Nothing is inlined: no
// data: URL, no inline script (the CSP of the shell allows 'self' only).
const hub = defineConfig({
  plugins: [svelte()],
  base: './',
  build: {
    outDir: 'dist',
    target: 'es2022',
    assetsInlineLimit: 0,
    modulePreload: { polyfill: false },
    sourcemap: false,
  },
  test,
});

// `vite build --mode mobile` builds the page of testing mobil into `dist-mobile/`, which the Tauri shell (mobile/)
// bundles: files of its own origin only, no inline script, as under the policy of the shell's WebView.
const mobile = defineConfig({
  plugins: [svelte()],
  base: './',
  build: {
    outDir: 'dist-mobile',
    target: 'es2022',
    assetsInlineLimit: 0,
    modulePreload: { polyfill: false },
    sourcemap: false,
    rollupOptions: {
      input: 'index.mobile.html',
    },
  },
  test,
});

export default ({ mode }: { mode: string }) => (mode === 'web' ? web : mode === 'mobile' ? mobile : hub);
