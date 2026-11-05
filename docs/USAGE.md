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
and adds a link. Without a download, from a checkout: `.\scripts\build-install.ps1` (Rust, Node with pnpm; `-BuildOnly`
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
testing-hub --version
```

The config folder is `%APPDATA%\testing-desktop` (`TESTING_DESKTOP_CONFIG_DIR` moves it). The default port is 17618
(`TESTING_DESKTOP_PORT`, or `port` in `config.json`). In [hub-desktop](https://github.com/WKDSteve/hub-desktop) the app is
the tile **Testing**: one click starts it and opens it.

## The page

The first page is the fleet: every repository with its last local run and the CI on each system, the red ones first, a
filter for the red ones and a search. A click opens one repository with its red cases and their output. A run in a
terminal shows by itself a moment after it ends: the hub watches the state folder and tells the page (a change of the CI
record too; a new commit or a changed `areas.json` shows within the regular scan of 45 seconds). The page also reads
again when the window gets the focus back. The CI columns say "not asked" for now: the hub does not ask GitHub yet.

More on the security of the server: [SECURITY.md](SECURITY.md). On releases: [RELEASING.md](RELEASING.md).
