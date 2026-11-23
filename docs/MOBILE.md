# testing mobil

The phone app of testing-desktop: the fleet green or red, the running jobs, the last red run with its cases, a notice
when a main turns red, and starting the failed jobs of a red run again. Android first (a Tauri 2 app), iOS later with
a Mac. It is a client of the hub on your PC and nothing more: the hub stays on loopback, the phone reaches it through a
tunnel, and the hub decides what the phone sees and may do. The parts every phone app of the family shares are
appkit's mobile basis ([appkit `docs/MOBILE.md`](https://github.com/WKDSteve/appkit/blob/main/docs/MOBILE.md)).

## What it does, and what it never does

| On the phone | |
|---|---|
| The fleet, a repository, its red cases and findings | read |
| Start the failed jobs of a red CI run of the default branch again | with the level `trigger` of this phone and a confirmation by its fingerprint or screen lock, each time |
| A notice when a main turns red | through Tailscale, with a UnifiedPush app on the phone |
| Run the test sheets of a task on the PC (from tasks mobil) | with the level `trigger` and a confirmation, each time; only a task a sheet of its repository names |
| Where a run stands | read; of a run that is not a task's sheets only that one is under way |
| A test sheet step by step, a run of a whole suite or of a file, the abort of a run, the event stream | never (they stay on the PC) |
| Releasing a commit for the self-hosted runner (`testing ci release`), trusting a root, any command | never: the release assistant is for a session of the PC and no route of it is released to a phone; no route of the hub trusts a root or runs a command (tests `phone.rs`, `crates/testing-hub/src/phone.rs`) |

A job the runner guard refused is not released from the phone: release its commit on the PC, where you look at what
it changes (`USAGE.md`, *Releasing a commit for the runner*). A run GitHub refused (`rejected`: it did not take the
workflow file, the run failed without a job) is not started again either: the phone says so, the workflow file is
fixed on the PC. The run of a task's test sheets (`POST /api/testing/run/task`, `{ "task": "<area>/<slug>" }`) is
testing.nvim's `testing sheets run --task` on the PC, in the repository of the fleet named like the task's area; the
hub starts it only when a sheet there names the task. tasks mobil calls it; the routes are in
[USAGE.md](USAGE.md#a-run-through-testingnvim).

## The three ways

- **Tailscale Serve** (recommended): on the PC, `tailscale serve --bg --https=443 http://127.0.0.1:17618`, and the
  name of the PC in the tailnet in `config.json` (`"tailnet_name": "pc.example.ts.net"`, or `TESTING_DESKTOP_TAILNET`).
  Through it only a paired phone gets a session. Never `tailscale funnel`. When another hub of the PC (tasks-desktop)
  already has 443: `tailscale serve --bg --https=8443 http://127.0.0.1:17618` and the name with that port,
  `pc.example.ts.net:8443` (never `:443`); the pairing link and the app carry the port.
- **SSH tunnel**: an SSH client on the phone (Termux, ConnectBot) forwards `127.0.0.1:17618` on the phone to the same
  port on the PC. `adb reverse tcp:17618 tcp:17618` does the same over USB (or adb over Tailscale) for a test.
- **An export on the phone**: clone a repository that holds an export of `testing export` with Git (MGit, Termux), then
  choose its folder in the app. Read only; it shows the state of the export, with its time.

## Pairing, levels, losing the phone

```
testing-hub phone pair                 a QR code to scan with the app (TESTING_DESKTOP_TAILNET, else config.json)
testing-hub phone pair --tunnel        the same for a phone behind an SSH tunnel
testing-hub phone list                 the paired phones: id, name, level
testing-hub phone level <id> trigger   let this phone start a rerun of a red run, and a task's test sheets
testing-hub phone level <id> read      no more
testing-hub phone revoke <id>          forget the phone; its sessions end at once
```

`phone pair` shows the code in the terminal, black on white whatever the theme, and in a terminal also as a picture in
a window (the one to scan when the terminal's font leaves gaps); the command ends when the code is over, after 60 s,
and removes that page. `--no-window` leaves the window out.

A new phone reads (`create`, the kit's default; this app has nothing to create). The app keeps two keys in the Android
Keystore: one to log in, and one that signs a rerun only after your fingerprint or screen lock. A phone without a screen
lock pairs and reads, and cannot start anything.

Lost the phone: `testing-hub phone revoke <id>` on the PC, and remove the phone from the tailnet in the Tailscale admin
console. The phone held the fleet of the repositories released for it, no key that opens anything without its lock.

## Notices

In the app: settings, "Tell me when a main turns red". It needs a UnifiedPush distributor app on the phone (Sunup, which
uses Mozilla's push service, or ntfy): the hub sends Web Push to it, encrypted for the app, with no account at any push
provider. A notice says that a main turned red and nothing more; the app shows which when you open it. While a phone
waits for notices, the hub reads the fleet every five minutes (the engine asks GitHub at most once a minute per
repository).

## Build (private machine only)

The shell is `mobile/` (a Cargo project of its own, Tauri 2), the page the third entry of `ui/` (`main.mobile.ts`,
`pnpm mobile:build` makes `ui/dist-mobile/`). It needs the Android SDK and NDK, Java 17 or newer, and the Android
targets of Rust (`rustup target add aarch64-linux-android x86_64-linux-android`).

```
cd ui && pnpm install --frozen-lockfile && pnpm mobile:build && cd ..
cd mobile
ANDROID_HOME=<sdk> NDK_HOME=<sdk>/ndk/<version> cargo tauri android build --debug --target aarch64 --apk
adb install -r gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

Never on the workstation (`MREL-06`). Not published anywhere yet: no store, no signed release (`MREL-01`, `MREL-02` come
before the first one).

## Signing and the development build

The development APK is built and signed by `scripts/android-sign-and-upload.ps1`, on the private machine only, with a
development key that is not the key of a later release. No key and no secret exists in GitHub.

- **The key** is a keystore file outside every repository (for example `%USERPROFILE%\.android-keys\testing-dev\`), made
  once with `keytool -genkeypair -keystore ... -alias testing-dev -keyalg RSA -keysize 4096 -validity 10950`; its password
  sits in the password manager and the file has a second copy offline. A lost key means: uninstall the app and install
  it again (a development build keeps nothing the hub cannot send again). The path is in
  `TESTING_ANDROID_DEV_KEYSTORE`; the script asks for the password when it signs and never writes it anywhere.
- **The run**: `pwsh scripts/android-sign-and-upload.ps1` builds the tip of `origin/main` in a temporary worktree,
  aligns and signs the APK, checks the signature (and the certificate, when `TESTING_ANDROID_DEV_CERT_SHA256` names it)
  and writes `android.json` and `SHA256SUMS-android` beside it. Nothing leaves the machine. With `-Upload` the files go
  to the pre-release `android-dev` of the repository, only for a commit whose CI is green, and the older APKs there
  are removed. It refuses on a workstation and without a key.
- **Why not `latest`**: the publish step of the rolling build removes every file that is not part of the build it
  publishes, so an APK on `latest` would be gone with the next green commit. The release `android-dev` is the one place
  for the development APK; install it with `adb install -r testing-mobile-dev-<sha>.apk`.
- **Test**: `pwsh scripts/test-android-sign.ps1` makes a throwaway key and a stand-in APK and checks the refusals, the
  signing and the certificate check. It never touches the real key and prints `n/a` where the Android tools are missing.
