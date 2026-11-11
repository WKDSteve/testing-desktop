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
| Data routes | `fleet`, `repo`, `finding`, `events`, `about` and the five `sheet-step` routes need a session; a route added to `ROUTES` is a data route by construction. The two POST routes (`sheet-step/start`, `sheet-step/command`) have the strict rate limit of a mutation. | matrix; `tests/routes.rs`; `tests/sheetstep.rs` |
| The engine only reads, except one run from the page | The hub asks `hello`, `fleet`, `repo` and `finding`, and writes into no repository. The one thing a session may start is a stepped run of a test sheet (`testing sheets step`), which writes no state and no snapshot. The page cannot name a call. | `src/engine.rs` tests; `tests/routes.rs`; `tests/sheetstep.rs` |
| No string from outside reaches the engine | `repo` and `finding` take the key of a repository; the parameters go to the engine as JSON on stdin, never in its arguments. | `tests/routes.rs`; `tests/engine.rs` (real engine) |
| A run from the page: only a known root, only a closed list | `sheet-step/start` takes the key of a repository of the fleet and the name of a sheet (letters, digits, `_ . -`; no separator, no leading `.` or `-`): the root is what the engine names for the key in the local form of `repo`, never a path of the request. `sheet-step/command` takes one of `next`, `continue`, `until`, `break`, `clear`, `answer`, `note`, `abort`; the hub builds the line on the child's stdin, a step number is a whole number, the text of an answer is one line of at most 400 printable characters, that of a note one line of at most 1000 bytes. The child gets the engine's allow-listed environment plus the trust of roots and the role of the machine from the hub's own environment, so testing.nvim's gates (trust, the workstation) hold as in a terminal; a refusal is the answer of `start`. Beyond that it gets only what the sheet engines and a program a sheet names need to be found (`TESTING_PLAYWRIGHT_DIR`, `TESTING_DESKTOP_DRIVER`, `PLAYWRIGHT_BROWSERS_PATH`, `CARGO_HOME`, `RUSTUP_HOME`). The root of the run is written `<path>` in every text of an event the page reads. | `src/sheetstep.rs` tests; `tests/sheetstep.rs` (a stand-in child that records its stdin); `tests/engine.rs` (the real engine: an untrusted root is refused) |
| A run leaves no path and no file of its own choosing on the page | `root` and `path` of `start`, `image` and `shot` of `step-after` are taken out before the page sees an event; a picture is asked for by size and step (`sheet-step/image?size=&n=&kind=`), and the hub serves only the file the child named for that step (PNG or JPEG, at most 16 MiB). A request that names a file is refused. | `tests/sheetstep.rs` (a file of the root the child never named is not served) |
| One run per hub, and it ends with the hub | A second start is refused and names the sheet that runs. `abort` and the stop of the hub tell the child `abort` and close its stdin (it then takes its own editor or browser down); a child that does not end within three seconds is killed. A child that writes a line over 1 MiB, or more events than a sheet can have, is ended. No request waits for the child except `start`, for its first event or its end, at most 20 seconds. | `tests/sheetstep.rs` |
| The event stream tells no content | An event of the fleet says that it changed and names no repository; the lines of a stepped run come on the same stream, as the page shows them. A session opens at most four streams, and a session that was closed takes its stream with it. | `tests/routes.rs`; `tests/sheetstep.rs` |
| A change costs one question, a request none | Only the three result files of a project in the state folder wake the scan, so what else writes there (and a page that reads often) does not start processes. | `src/watch.rs` tests; `tests/routes.rs` |
| Data classes fail closed | The hub reads the export form: no absolute path, and only the `public` and `internal` repositories that the `areas.json` of the device names. Without the file none is shown; a private one never. | `tests/engine.rs`; `e2e/app.spec.ts` |
| The GitHub token stays in the hub | From `github_token_file` (for the user only; on other rights, a broken file or a wrong path the start stops) or one `gh auth token` at the start, without a shell and with a time limit. It goes to the engine only in the requests of `fleet` and `repo`, as JSON on stdin: never in an argument or the environment of the child. No answer holds it, and its type has no printable form. `about` says where it came from as a word. | `src/github.rs`, `src/engine.rs`, `tests/routes.rs`, and the specs of testing.nvim |
| GitHub is asked at a fixed rate | However often a page reads, the engine asks GitHub at most once a minute per repository (`remote_max_age`, the stored answers of its CI adapter), with ETags. | `src/github.rs`, `tests/routes.rs`, and the specs of testing.nvim |
| Output of a test is text | Names and output of red cases are written by whatever ran: they are shown as text, never as markup or a link, and hidden and reordering characters are made visible (`⟦U+202E⟧`). | `e2e/app.spec.ts` (a repository with markup, a script link, reordering characters and very long output) |
| The page's policy | The policy of appkit for every page; nothing of this page breaks it. | the matrix; `e2e/app.spec.ts` (no violation in any test) |
| Nothing of a result in the browser | The session is in `sessionStorage` of the tab, and `localStorage` stays empty. | `e2e/app.spec.ts` |
| Accessibility | axe reports nothing on the views in the light and the dark scheme. | `e2e/app.spec.ts` |
| Phones (testing mobil) | A phone pairs once with a key of its own (appkit's devices), comes through Tailscale Serve (`tailnet_name`) or a tunnel to loopback, and reaches only `fleet`, `repo`, `finding` and `about` (level `read`) and `ci-rerun` (level `trigger`, confirmed on the phone per request); every other route, the five `sheet-step` routes and `events` among them, answers it `not_for_devices`. No route of the hub trusts or releases a commit or runs a command of the request. | `src/phone.rs` tests; `tests/phone.rs` (the matrix with phones, the tailnet, a phone that reads and reruns only confirmed) |
| `ci-rerun` | Takes the key of a repository of the fleet and nothing else; starts the failed jobs of the run the fleet names for it again with `gh run rerun <id> --failed -R <owner>/<repo>` (an argv, no shell, input closed, 30 s), only when that run failed, was cancelled or found no runner. A run the runner guard refused (`rejected`) is not started again: that is released on the PC. | `src/rerun.rs` tests; `tests/phone.rs` |
| Notices | While a phone has subscribed (appkit's Web Push), the hub reads the fleet every five minutes and tells it when a repository turns red: the kind and its key, nothing of a case. | `src/alarm.rs` tests |

## The web variant

`testing.html` has no server to guard; what holds for it:

| Thing | What holds | Where it is tested |
| --- | --- | --- |
| No request | Its policy forbids every request (`connect-src 'none'`), with no WASM and no worker; the sources name no way to one. | `ui/web/web.test.mjs`, `ui/e2e-web/` (every test counts them) |
| No `eval` | The check of the documents is compiled ahead of time from the schema of the contract. | `ui/web/web.test.mjs` |
| Never half an export | Every file is checked against its size and sum in the manifest and against the contract before anything of the export is used; a read that fails leaves the page on the export it holds, and says so. A higher version of the contract is refused by name. | `ui/web/folder-source.test.ts`, `ui/e2e-web/` |
| Only the export form | A document that is not in the export form (it could hold absolute paths) is refused. | `ui/web/folder-source.test.ts` |
| One database | Every local file shares the origin `file://`: the page opens only its own database `testing-web`, for the handles of its folders, and never another page's. Nothing of an export is kept in the browser. | `ui/web/web.test.mjs`, `ui/e2e-web/` |
| Foreign text | As on the hub: text, never markup; a link only to a CI run of GitHub. | `ui/e2e-web/` |

That the browser keeps a folder over a restart (the fake folder of the tests does not clone) is checked by hand.

## What it does not do

- It is not a defence against someone who already runs code as you: they can read `control.secret` and the state of
  testing.nvim.
- It does not run the suite of a repository, and it does not write into a repository. A stepped run of a test sheet
  from the page is the one thing it starts: testing.nvim's own command, in a repository of the fleet, under
  testing.nvim's gates, writing no state. The other is GitHub's: the failed jobs of a red CI run started again
  (`ci-rerun`), which runs on GitHub's runners or yours under the runner guard, as a rerun from the web page would.

## The CI of this repository

The workflows run on the maintainer's own machine. They start only on a push to `main`, by hand, on a pushed tag, or on
a finished CI run of such a push; none has a `pull_request` or `pull_request_target` trigger, and a fork pull request
needs the maintainer's approval (`docs/RELEASING.md`, *The runner*). Actions are pinned by commit.

## Reporting

A hole in the guard belongs to appkit (see its `SECURITY.md`); one in what this page lists belongs here. Open an issue
without the details, or write to the maintainer.
