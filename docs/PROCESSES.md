# Programs testing-desktop starts

Every program the hub (`testing-hub`) starts, with why, how long it lives and which way of
[appkit's `docs/PROCESSES.md`](https://github.com/WKDSteve/appkit/blob/main/docs/PROCESSES.md)
(rule `RS-08`) starts it. **A process that is not in this list is a bug.** The list is taken
from the code (`tools/process-starts.allow`, the `appkit_core::child` and `launch` calls of this
repository and of appkit) and checked against a measurement with `app-proc-watch`.

The ways:

- **child**: `appkit_core::child`. No console window, the streams are pipes of the hub, bound to
  the hub by the system (a job object on Windows, a process group elsewhere): it ends with the
  hub, also when the hub crashes or is killed, together with everything it started. Its start
  is in `hub.log`.
- **system**: the system opens something (`ShellExecuteW` on Windows); no process of the hub is
  started. On macOS and Linux `/usr/bin/open` or `xdg-open` is started through
  `appkit_core::launch`.

| Program | Why | How long | Way |
| --- | --- | --- | --- |
| `testing-hub serve` | `start` (the link, the login entry, `install`) puts the hub in the background | until `stop`, the stop route, or the hub ends itself | named exception: the hub in the background (`background::start`), no window, output to `hub.log` |
| `nvim --version` | the start checks the version of Neovim | the call | child |
| `nvim --headless -u NONE -i NONE -n -l <testing.nvim>/scripts/testing.lua call <method>` (the engine) | every question of the page (the fleet, a repository, the export); one process per question, no engine host | the call | child |
| `nvim … testing.lua run --root <root> [--file <name>]`, `sheets run --task <id> --root <root> --result` | **Run** in the page: the suite, one file, the sheets of a task | at most 5 hours, or **Abort**; ends with the hub together with the tests it started | child (`child::spawn`) |
| `nvim … testing.lua sheets step <sheet> --root <root>` | a step of a test sheet driven from the page | until the sheet ends, **Abort**, or the hub ends | child |
| `nvim … testing.lua ci release …` | the release assistant of the runner guard, one step at a time | 25 seconds for a reading step, 10 minutes for a release | child |
| `gh auth token` | the sign-in to GitHub for the CI view, when no token is set | the call, at most 10 seconds; never asks anything (`GH_PROMPT_DISABLED=1`) | child |
| what the engine starts (the tests of a run, `git`, `gh`) | the engine's own work | in the job of its engine process: it ends with the hub; what a single call (`call <method>`) started on purpose for you, such as an editor, stays (`Leftovers::Keep`) | child of a child |
| `<installed testing-hub> start` | `testing-hub install` brings the hub back from the installed program | the call; the hub it puts in the background stays | child (`background::start_from`, `Leftovers::Keep`) |
| the default browser | `start --open`, `open`, the link | yours | system (`appkit_hub::browser::open`) |
| `conhost.exe` | Windows gives every console program (`nvim`, `gh`, `git`) a console host, also without a window | as long as its console program | comes with each child; no window |

What the hub does **not** start any more: `taskkill.exe` (`stop` asks the hub over its control
route and ends it with `TerminateProcess` only when it does not answer), `reg.exe` (the login
entry and the uninstall entry go through the registry API), `icacls.exe` (the rights of the
control secret go through the security API), `cmd.exe` or a script host.

The release build is a window program (GUI subsystem): the link, the login entry and a double
click open no console window. `testing-hub setup` opens a console window only for its questions.
