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

A failed step pauses so you can look at it; a browser sheet opens its window beside the page. The run writes no state
and no snapshot. What testing.nvim refuses (the workstation rule, a root that is not trusted), the page shows with the
reason: trust comes from `TESTING_TRUST_ROOTS` of the hub's environment, as in a terminal. Leaving the page (the menu
**View › All repositories**) does not end the run; **View › Step by step** comes back to it, and the hub's own stop
ends it.

More on the security of the server: [SECURITY.md](SECURITY.md). On releases: [RELEASING.md](RELEASING.md).
