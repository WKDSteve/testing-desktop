# Security of the local server

`testing-hub` is a web server on your own machine. Anything on the machine, and any web page you open in a browser, can
try to talk to it. The guard, the pairing, the sessions, the limits and the engine seam are those of
[appkit](https://github.com/WKDSteve/appkit): **its `SECURITY.md` is the description of what holds for every app, and
its security matrix (`appkit-testkit`, `security_matrix!`) is the shared test of it.** This hub runs that matrix
(`crates/testing-hub/tests/security.rs`): every route of `ROUTES` is a data route, and the matrix tries each one without
a session. This page says what is the testing hub's own.

## What is the testing hub's own

| Thing | What holds | Where it is tested |
| --- | --- | --- |
| Names | App, program, variables, port, routes and login entry are the hub's own and share none with `tasks-hub`, `rules-hub` or the demo app. | `tests/security.rs` |
| Data routes | `fleet`, `repo` and `about` need a session; a route added to `ROUTES` is a data route by construction. | matrix; `tests/routes.rs` |
| The engine only reads | The hub asks `hello`, `fleet` and `repo`; it starts no test run and writes into no repository. The page cannot name a call. | `src/engine.rs` tests; `tests/routes.rs` |
| No string from outside reaches the engine | `repo` takes the key of a repository; the parameters go to the engine as JSON on stdin, never in its arguments. | `tests/routes.rs`; `tests/engine.rs` (real engine) |
| Data classes fail closed | The hub reads the export form: no absolute path, and only the `public` and `internal` repositories that the `areas.json` of the device names. Without the file none is shown; a private one never. | `tests/engine.rs`; `e2e/app.spec.ts` |
| Output of a test is text | Names and output of red cases are written by whatever ran: they are shown as text, never as markup or a link, and hidden and reordering characters are made visible (`⟦U+202E⟧`). | `e2e/app.spec.ts` (a repository with markup, a script link, reordering characters and very long output) |
| The page's policy | The policy of appkit for every page; nothing of this page breaks it. | the matrix; `e2e/app.spec.ts` (no violation in any test) |
| Nothing of a result in the browser | The session is in `sessionStorage` of the tab, and `localStorage` stays empty. | `e2e/app.spec.ts` |
| Accessibility | axe reports nothing on the views in the light and the dark scheme. | `e2e/app.spec.ts` |

## What it does not do

- It is not a defence against someone who already runs code as you: they can read `control.secret` and the state of
  testing.nvim.
- It does not ask GitHub: the CI columns say "not asked", and the hub has no token.
- It does not run tests, and it does not write into a repository.

## The CI of this repository

The workflows run on the maintainer's own machine. They start only on a push to `main`, by hand, on a pushed tag, or on
a finished CI run of such a push; none has a `pull_request` or `pull_request_target` trigger, and a fork pull request
needs the maintainer's approval (`docs/RELEASING.md`, *The runner*). Actions are pinned by commit.

## Reporting

A hole in the guard belongs to appkit (see its `SECURITY.md`); one in what this page lists belongs here. Open an issue
without the details, or write to the maintainer.
