# Using testing-desktop

`testing-hub` shows the test and CI results of the repositories that use
[testing.nvim](https://github.com/WKDSteve/testing.nvim), in your browser, from a server on your own machine. It only
reads: the results are what testing.nvim has written.

## Install

From a release or the rolling build, in PowerShell (not elevated, no administrator rights):

```
irm https://github.com/WKDSteve/testing-desktop/releases/download/latest/install.ps1 -OutFile install.ps1
.\install.ps1
```

The script downloads the program, checks it against `SHA256SUMS` and hands over to `testing-hub install`, which copies
it to `%LOCALAPPDATA%\testing-desktop\bin`. `-InstallArgs '--autostart','--shortcut'` also registers it with your login
and adds a link. It ends with the whole call that starts it (`start it: "...\bin\testing-hub.exe" start --open`) and says
whether that folder is on `PATH`: install does not add it there, so `testing-hub` by name works only once you add the
folder yourself; the Start menu entry **Testing** starts it too. Without a download, from a checkout: `.\scripts\build-install.ps1` (Rust, Node with pnpm; `-BuildOnly`
only builds).

Remove it with `uninstall.ps1` (`scripts/uninstall.ps1`, or the file of the release). Your settings stay unless you add
`-PurgeSettings`; your repositories and the results of testing.nvim are never touched.

## Without an install: the web variant

`testing.html` is the same page as one file you open from disk (`file://`), for a machine that may not install
anything. There is no server and it makes no request. It reads a folder that `testing export --out <dir>` wrote:
it shows the fleet and the red cases of that export, and how old it is. It starts no run, asks GitHub nothing and
writes nothing; the CI columns hold what the export holds. Build it from a checkout with `pnpm web:build` in `ui/`
(`ui/dist-web/testing.html` and its `SHA256SUMS`). Chrome and Edge keep the folder and look at it again when the
window gets the focus back and every 30 seconds. Firefox and Safari have no folder picker: there the folder is read
once, through a file dialog.

### A recorded run, step by step

The same page plays back a test sheet that ran step by step (`testing sheets step`, `:Testing sheet step`, or **Step
through** on the hub's page). testing.nvim records every such run beside its pictures: the folder
`testing/sheet-evidence/<sheet id>` below the cache folder of Neovim (`%TEMP%\nvim` on Windows, `~/.cache/nvim`
elsewhere; `:echo stdpath('cache')` says where), with `events.jsonl` and the pictures of the steps. Copy that
folder to where the browser may read it (a browser may refuse a system folder such as `AppData`, where the cache
folder is on Windows) or to another machine, and
choose it with **Choose folder**: a folder with `events.jsonl` and no export in it is a recording.

It opens at the end of the run, with the view of the hub's "Step by step" page: the steps with their state, and the
tile of a step with what it did, its notes, the reason of a failure, the screen of the child editor as text or the
picture of the target. **To the start**, **Step back**, **Step forward** and **To the end** (`s`, `p`, `n`, `e`) walk
through it: the page shows the run as it stood after that many steps. Nothing runs and nothing is sent: there is no
live view in the web variant, a run is followed live on the hub's page. The recording names no folder of the machine
it was made on. `notes.json` in the same folder does (the root of the repository), and the page does not read it: to
hand a run on, `events.jsonl` and the pictures are enough. The folder holds the last stepped run of a sheet; the next
one replaces it.

## What it needs

- **Neovim 0.12 or newer**: `TESTING_NVIM`, else the one on the `PATH`, or the key `nvim` of `config.json`.
- **A checkout of testing.nvim and of lib.nvim**: `TESTING_ENGINE_DIR` and `LIB_NVIM_DIR`, else below `REPOS_DIR`, or the
  key `engine_dir` of `config.json`.
- **The folder of your repositories**: `vault` of `config.json`, else `TESTING_REPOS_ROOT`, else `REPOS_DIR`.
- **The data classes of this device**: `areas` of `config.json` or `TESTING_DESKTOP_AREAS`, by default the `areas.json`
  of tasks-desktop. Only the `public` and `internal` repositories it names are shown; without the file none is.
- **A GitHub token, for the CI columns** (optional): `github_token_file` of `config.json` names a file with a read-only
  token (Actions and Metadata read; on Linux and macOS the file must be for you only), else the hub takes `gh auth token`
  once at its start. Without either the columns say "not asked", and Help says why. `"ask_github": false` switches it off.
  The hub asks GitHub at most once a minute per repository, however often a page reads.
- **The state of testing.nvim**: `state_dir` of `config.json` or `TESTING_DESKTOP_STATE_DIR`, by default the state folder
  of Neovim (`%LOCALAPPDATA%\nvim-data`).

What is missing, the page says. `about` says where each setting came from, never the path.

## The commands

```
testing-hub                       (a double click) start the hub in the background, open the browser
testing-hub serve [--port N]      run the hub until Ctrl-C
testing-hub start [--open]        run it in the background (log in the config folder)
testing-hub stop                  stop that hub
testing-hub open [--launch]       print (or open) a one-time pairing link
testing-hub autostart on|off|status
testing-hub install [--autostart] [--shortcut]
testing-hub uninstall [--purge-settings]
testing-hub phone pair [--tailnet <name> | --tunnel]   pair testing mobil (a QR code)
testing-hub phone list | level <id> <level> | revoke <id>
testing-hub --version
```

The phone app, its ways to the hub and what it may do: [MOBILE.md](MOBILE.md).

The config folder is `%APPDATA%\testing-desktop` (`TESTING_DESKTOP_CONFIG_DIR` moves it). The default port is 17618
(`TESTING_DESKTOP_PORT`, or `port` in `config.json`). In [hub-desktop](https://github.com/WKDSteve/hub-desktop) the app is
the tile **Testing**: one click starts it and opens it.

`testing-hub open` without a running hub says so and names `testing-hub start --open`, which starts it and opens the
page in one go.

## The page

The hub reads the fleet as soon as it starts, before a page asks: right after a start that read also asks GitHub and
takes up to a minute, so a hub that starts with your login has it ready long before you open the page. A page that
comes earlier says that the hub is reading the results, not that it is still connecting.

The first page is the fleet: every repository with its last local run and the CI on each system, the red ones first, a
filter for the red ones and a search. A click opens one repository with its red cases and their output. A run in a
terminal shows by itself a moment after it ends: the hub watches the state folder and tells the page (a change of the CI
record too; a new commit or a changed `areas.json` shows within the regular scan of 45 seconds). The page also reads
again when the window gets the focus back. The word in a cell ("green", "3 failed", "unknown", "not asked") is explained
for its repository when the pointer rests on it, for example "The last CI run of images.nvim on ubuntu failed."; a
screen reader reads the sentence after the word.

### A run through testing.nvim

The page of a repository starts a run of testing.nvim, as on the command line: **Run the suite**
(`testing run --root <repo>`) or **Run these files** with a part of a spec file name (`--file <text>`). One run at a
time per hub; the page follows it to its verdict (green, red, refused by testing.nvim, could not be made, stopped) and
shows the end of what testing.nvim wrote, with the folders of the PC taken out. A whole suite is a heavy run: it waits
for its turn in testing.nvim's queue of heavy runs of this machine, behind a run in a terminal or of another session.
Its result goes into the state folder the hub reads (`--state-dir`), so the repository shows it as its last local run.
What testing.nvim refuses (a root that is not trusted, the workstation) is the verdict "refused".

The test sheets bound to a task (a sheet with `task = "<area>/<slug>"`) run the same way through
`testing sheets run --task`; that is the run a paired phone may start (tasks mobil), confirmed on the phone. The hub
finds the sheets in the repository of the fleet named like the task's area, and starts nothing for a task no sheet
names. The routes, for a program that starts a run (the central hub, tasks mobil):

| Route | Body or query | Answer |
| --- | --- | --- |
| `GET /api/testing/run` | none | `{ "schema": 1, "kind": "hub.run", "available": bool, "run": null \| { "id", "key", "what": "suite" \| "file" \| "task", "file"?, "task"?, "running", "exit", "verdict", "started", "ended", "output" } }`; a phone sees a task's run without `output`, any other only as `{ "what": "other", "running" }` |
| `POST /api/testing/run/start` | `{ "key": "<key of the fleet>", "file"?: "<part of a spec file name>" }` | `202` and the state; `409 busy` while a run is under way. The PC only |
| `GET /api/testing/run/task-sheets` | `?task=<area>/<slug>[&key=<key of the fleet>]` | `{ "schema": 1, "kind": "hub.task-sheets", "task", "key", "sheets": [<name>...] }` |
| `POST /api/testing/run/task` | `{ "task": "<area>/<slug>", "key"?: "<key of the fleet>" }` | `202` and the state; `404 no_sheet` (no sheet names the task), `404 no_repo`, `409 ambiguous` (name the repository with `key`), `409 busy`. A phone at the level `trigger`, with `x-device-confirm` |

### A test sheet step by step

A repository lists its test sheets (the files below its `TESTS/sheets`; see `docs/SHEETS.md` of testing.nvim). **Step
through** runs one the way a debugger runs a program: the hub starts testing.nvim's `testing sheets step` as a child
and the page is its client, the same as `:Testing sheet step` in the editor, with the same words. One run at a time per
hub.

- Left: the steps of the size that runs, each with its state (not yet, next, runs, pass, fail, not run), its number,
  its action and its note, and a breakpoint dot in front of it. A click on a step chooses it.
- Right: the tile of the chosen step (else of the last one that did something or got a note): what it does, how long
  it took, its notes, the reason of a failure, for a Neovim sheet the screen of the child editor as text, for a browser or desktop sheet the
  picture of the target after the step.
- The buttons, and the same keys as in the editor: **Next** (`n`) runs this step; **Continue** (`c`) runs on to the
  next breakpoint, a failed step or the end; **Until here** (`u`) runs on to the chosen step; **Breakpoint** (`b`)
  sets or removes one on the chosen step, in every size; **Abort** (`a`) ends the run.
- **Add note** (`N` goes to the field) keeps a line on the step where the run stands (before it, after a failure the
  failed one), at any time while it runs; at most 1000 bytes. testing.nvim keeps the notes of the last run for
  `testing sheets report`.
- A question of the sheet (`ask`) stops the run and shows the question with **Yes** (`y`), **No** and a note; a no
  fails the step with the note as its reason.
- The last line says where the run stands: paused before a step, a failed step, the question, or `finished: pass`.
  A run that went through without an abort counts as a verdict, as the line under it says.
- **Explain this step with AI**, under the tile of a step, is closed until you open it. It offers the three questions
  of `E` in the editor's panel: what the step does, what to watch out for, and (once the step ran) what its result
  means. A click asks the run for the request an AI would get: testing.nvim builds it from what the run reported about
  the step (the sheet, its steps, what this one did, the screen of the child editor as text, its notes; no picture),
  and the page shows all of it with where it goes. From this page it goes nowhere: the hub cannot send a request yet
  (it will go through ai.nvim, once that can be asked without an editor). **Copy** hands it to the AI you choose. A
  request is built while the run is paused or waits for an answer, which is where a failed step stops. After the end
  of a run there is none, and the web variant builds none from a recording.

A failed step pauses so you can look at it; a browser sheet opens its window beside the page. The run writes no state
and no snapshot. What testing.nvim refuses (the workstation rule, a root that is not trusted), the page shows with the
reason: trust comes from `TESTING_TRUST_ROOTS` of the hub's environment, as in a terminal. Leaving the page (the menu
**View › All repositories**) does not end the run; **View › Step by step** comes back to it, and the hub's own stop
ends it.

### Releasing a commit for the runner

The runner guard of testing.nvim keeps foreign code off a self-hosted runner: a job of a public repository runs only
for a commit you released (`docs/RUNNER-GUARD.md` of testing.nvim). A job it refused shows as a red CI run. **View ›
Release a commit for the runner…** (or **Release a commit…** on the page of a repository whose CI run failed) walks
through the steps of `testing ci release`, the same as in a terminal:

1. **Repository**: the repositories of the fleet that GitHub knows and those on the guard's list, a red CI first.
2. **Commit**: the last commits of the default branch, each with whether it is released.
3. **What changed**: since the last released commit: the commits, the files, and marked those that decide what the
   runner executes (the workflow, scripts, lock files); a warning when nothing was released before, when the history
   was rewritten, or when the changes could not be read. Tick **I looked at these changes** to go on.
4. **Release**: one question, with the full commit id.
5. **Result**: the commit is on the list, and its failed runs were started again (or what went wrong). The hub
   releases in the background: closing the page meanwhile does not stop it.

testing.nvim does every step (`testing ci release ... --json`): it asks GitHub through `gh` with your own login of
`gh`, keeps the list (`reviewed.json` below `TESTING_RUNNER_ROOT`, else `~/actions-runners`), and releases. The page
only shows what it says. A release takes the full commit id of the review aid you were shown, within fifteen minutes;
`testing ci trust --revoke <owner/repo> <sha>` in a terminal takes an entry back. It needs a testing.nvim that has the
steps as documents (`testing-ci-release/1`); an older one makes the page say so. A phone never reaches the assistant.

The web variant has no assistant: for a red CI run of an export it names the command, `testing ci release
<owner/repo> <sha>`, to copy into a terminal on the PC.

The routes, for a program on the PC:

| Route | Query or body | Answer |
| --- | --- | --- |
| `GET /api/testing/release/repos` | none | `{ "schema": 1, "kind": "hub.release.repos", "exit", "problem", "repos": [{ "name", "listed", "mode", "reviewed", "ci", "red" }] }` |
| `GET /api/testing/release/commits` | `?repo=<owner/name>` | `{ ..., "kind": "hub.release.commits", "repo", "commits": [{ "sha", "title", "author", "date", "released" }] }` |
| `GET /api/testing/release/show` | `?repo=<owner/name>&sha=<40 hex>` | `{ ..., "kind": "hub.release.show", "repo", "sha", "show": null \| { "already", "prev", "compared", "problem", "status", "total_commits", "commits", "files": [{ "path", "previous", "status", "flag" }], "files_total", "truncated" } }` |
| `POST /api/testing/release` | `{ "repo": "<owner/name>", "sha": "<40 hex>" }` | `202` and `{ "schema": 1, "kind": "hub.release.started", "repo", "sha" }`: the release runs in the background, to its end whatever becomes of the page; `409 not_shown` without the review aid of that commit read just before, `404 unknown_repo` for a repository `release/repos` did not name, `409 busy` |
| `GET /api/testing/release/outcome` | none | `{ "schema": 1, "kind": "hub.release.outcome", "state": "none" \| "pending" \| "done" \| "failed", "repo", "sha", ... }`: for `done` the engine's `exit`, `problem` and `"release": null \| { "already", "recorded", "reruns": [{ "id", "name", "was", "ok", "problem" }] }`, for `failed` `"error": { "code", "message" }` (the step gave no document) |

`exit` and `problem` are the engine's: `0` done, `2` it refused the arguments, `3` GitHub or the list failed. A reading
step answers within 25 seconds or says that GitHub took too long. The release is not tied to its request (the hub
gives a request 30 seconds, a release that starts many runs again takes longer): while it runs every other step is
`409 busy`, and the page asks `release/outcome` until it is over.

More on the security of the server: [SECURITY.md](SECURITY.md). On releases: [RELEASING.md](RELEASING.md).
