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
| A test sheet step by step, the event stream | never (they stay on the PC) |
| `testing ci trust`, releasing commits of others (`testing ci release`), any command | never: no route of the hub does it, none is released to a phone (tests `phone.rs`, `crates/testing-hub/src/phone.rs`) |

A run the runner guard refused (`rejected`) is not started again from the phone: release it on the PC, where you look
at the commit. A run of a task's test sheet, started from tasks mobil, is a later step (task
`testing-mobil-blattlauf-einer-task-aus-tasks-mobil-web-ueber`).

## The three ways

- **Tailscale Serve** (recommended): on the PC, `tailscale serve --bg --https=443 http://127.0.0.1:17618`, and the
  name of the PC in the tailnet in `config.json` (`"tailnet_name": "pc.example.ts.net"`, or `TESTING_DESKTOP_TAILNET`).
  Through it only a paired phone gets a session. Never `tailscale funnel`.
- **SSH tunnel**: an SSH client on the phone (Termux, ConnectBot) forwards `127.0.0.1:17618` on the phone to the same
  port on the PC. `adb reverse tcp:17618 tcp:17618` does the same over USB (or adb over Tailscale) for a test.
- **An export on the phone**: clone a repository that holds an export of `testing export` with Git (MGit, Termux), then
  choose its folder in the app. Read only; it shows the state of the export, with its time.

## Pairing, levels, losing the phone

```
testing-hub phone pair                 a QR code to scan with the app (the tailnet name of config.json)
testing-hub phone pair --tunnel        the same for a phone behind an SSH tunnel
testing-hub phone list                 the paired phones: id, name, level
testing-hub phone level <id> trigger   let this phone start the failed jobs of a red run again
testing-hub phone level <id> read      no more
testing-hub phone revoke <id>          forget the phone; its sessions end at once
```

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
