# Releasing

A release is a deliberate act that ends with a person trying the result. The workflow
[`release.yml`](../.github/workflows/release.yml) makes a **draft**; nothing is published by a
machine.

## What a release holds

| File | What |
| ---- | ---- |
| `testing-hub-X.Y.Z-x86_64-pc-windows-msvc.zip` | the program for Windows |
| `install.ps1` | the install script of appkit's release-kit, stamped for this tag: it installs exactly this release |
| `uninstall.ps1` | the script of `scripts/` at this tag |
| `SHA256SUMS` | the checksum of every file above |

(There is no Linux or macOS archive: they would need hosted runners, and the builds run on a self-hosted Windows
runner. See *The runner* below. `scripts/uninstall.sh` and `scripts/test-install.sh` are tried in CI under Git Bash, but
are not part of a release.)

The native shell with a Setup is a later task; this workflow gets a `bundle` job when it exists. The scripts stay in
any case: they are the way for a person without administrator rights or with a program that is not signed.

The rolling build `latest` ([`rolling.yml`](../.github/workflows/rolling.yml)) carries the same
script, stamped for `latest`, beside its Windows program. `rolling.yml` takes the actions `prepare`
and `publish` of the release-kit, `release.yml` takes `publish` (its own `check` decides), both by
the commit of the appkit tag that `Cargo.lock` pins; `tools/check-appkit-pin.py` fails when the two
disagree, so a raised appkit tag raises them too.

## Steps

1. **Main is green** (the `ci` workflow, all jobs), the engine pin `engine.lock` is the one you mean.
2. **Bump the version** in `crates/testing-hub/Cargo.toml`, in its own commit, and push it. The tag
   must be `v` plus exactly that version; the workflow refuses otherwise.
3. **Wait for the CI of that commit.** The workflow looks for a green `ci` run of the very commit
   that the tag points at and stops without one.
4. **Tag and push:** `git tag vX.Y.Z && git push origin vX.Y.Z`. The workflow
   - checks the tag, the version, that the commit is on `main`, the green CI, and that no published
     release of the tag exists (`check`),
   - builds the interface and then the program for Windows, packs it, unpacks it again in
     another folder to ask it for its version, and runs the install script of the release-kit and
     the uninstall script against it in a folder of their own (`build`, rule `NREL-12`),
   - makes the draft with the notes of `.github/release-notes.md` (`{tag}` in them becomes the tag; the
     notes stay on the page when it is published, so they say nothing about being a draft) and adds the
     archive, the stamped install script, the uninstall script and `SHA256SUMS` (`publish`).
5. **Try the draft, in one command.** `scripts/try-release.ps1 -Tag vX.Y.Z` takes the files of the release
   (through `gh`, which may read a draft), checks them against `SHA256SUMS`, installs the archive with the
   `install.ps1` of the release, asks the installed program for its version, starts the hub, asks it for a
   pairing link and its page, stops it and installs once more (the repair). It prints one line per step and
   ends with exit 1 when one failed. The release stays installed; `-Uninstall` also tries `uninstall.ps1`
   (twice: the second run must find nothing to do), `-PurgeSettings` with it removes the settings too. It
   works on the real installation of the user: a hub that runs is stopped. Do this in a normal terminal of
   the user, not from an agent session (`WIN-03`): a green build is not a program that was tried (`NREL-07`).
   A build machine that runs elevated needs `-AllowElevated`; a user session does not and should not. The
   download path of `install.ps1` (no `-File`) is tried after publishing, once, with the line of the notes.
6. **Publish the draft** within about a day (`NREL-09`). If the tree moved on, delete the draft and
   the tag and cut again from `main`.
7. A **published release is never cut again** (`NREL-10`): a mistake is fixed in the next patch
   version.

## The web variant

[`web.yml`](../.github/workflows/web.yml) builds `testing.html`, tests it (the checks of the page, then the built file
from disk in Chrome) and only then puts it with its `SHA256SUMS` into the public repository
[testing-web](https://github.com/WKDSteve/testing-web). Unlike a release **there is no draft: the push is public at
once.** It runs on every tag `v*`, so the tag of step 4 publishes the page too, and by hand on `main`:

```
gh workflow run web.yml --ref main
```

A page that did not change is not committed again. It pushes with the git credentials of the runner, there is no secret
in the workflow. testing-web holds the page, its sums and its README, never an export. After the first run: the README of
testing-web no longer says "Not built yet", and the tile of hub-desktop gets its web variant
(`crates/hub-desktop/src/catalog.rs`).

## When something is wrong

- *the tag says X but Cargo.toml says Y*: bump the version (step 2) and tag again; delete the tag
  first (`git push origin :refs/tags/vX.Y.Z`).
- *no green CI run for the commit*: the tag points at a commit CI has not passed. Wait, or fix.
- *a published release exists already*: it is never cut again (step 7); make a patch release. A draft
  of the same tag is reused: its files and notes are replaced by this cut.
- *a file is missing from the sums*: the `build` job failed; read it, delete the draft, tag again.
- *nothing happens after the tag*: the runner is not running; `runners.ps1 status testing-desktop`.

## The rolling build

After every green `ci` run on `main`, `rolling.yml` builds the program of exactly that commit (the
interface first, then `cargo build --release --locked`) for Windows and replaces the files of
the pre-release `latest`. It is for looking at the newest state with a click, not a release: no
version tag, not signed. `testing-hub --version` names the commit (`testing-hub 0.1.0 (build abc1234)`).
Only the tip of `main` is built, and a newer tip replaces a build that is still running (a late CI run
of an older commit lands in a group of its own and builds nothing). The page of the release says what
it is: `.github/rolling-notes.md`.

## The engine pin

`engine.lock` names the commits of `testing.nvim` and `lib.nvim` this app is built and tested
against. CI checks both out at exactly these commits, so a typo in a pin fails the build; with
`TESTING_HUB_REQUIRE_ENGINE=1` a missing engine is a failure, not a skip (rule `TST-16`). The golden documents of
`crates/testing-hub/tests/fixtures/golden` and `ui/src/lib/contract.d.ts` are copies of the pinned commit's
(`tests/golden_copy.rs`). Moving a pin is its own commit, with the copies taken over and the tests green against the
new engine.

## What is not tried

- `scripts/test-install.*` run only in CI: from a Claude session the app redirects AppData and HKCU, so `install`,
  `start` and `autostart` are never started there, and a clean Windows account is step 5 of a release.
- Without an engine the tests of the real engine say `skipped` and prove nothing: that is the run without
  `TESTING_HUB_REQUIRE_ENGINE=1` on a machine without testing.nvim. CI is the evidence.
- Only Windows is built and tested; the paths for Linux and macOS in the program and in `uninstall.sh` have no CI run.

## Shared kit (`appkit`)

The parts shared with tasks-desktop and rules-desktop (server guard, sessions, shell, design tokens, and the release
kit) live in the repository [appkit](https://github.com/WKDSteve/appkit):

- the Rust parts come in as a **Git dependency with a tag** (never a branch), so `Cargo.lock` pins
  the exact commit; `cargo deny` allows that one Git source by name in `deny.toml`;
- the interface parts come in by the same rule, as a tagged Git dependency in `ui/package.json`;
- the actions of the release-kit in the two workflows are pinned by the full commit of the same tag;
- a change in `appkit` is released there first, then taken over here by raising the tag in all
  these places, in its own commit (`cargo update -p appkit-hub`, `cd ui && pnpm install`,
  `python tools/check-appkit-pin.py`). All of it for all apps in one command, from the clone of appkit:
  `python tools/bump-apps.py vX.Y.Z` (`--dry-run` to look first, `--commit` to commit each app, it never pushes).

## The runner

CI (`ci.yml`), the rolling build (`rolling.yml`) and the release (`release.yml`) all run on the maintainer's
self-hosted Windows runner (`runs-on: [self-hosted, windows]`, Git Bash by its full path). That is a choice, not a
limit: the builds use the same Neovim, Chrome and toolchain the app is used with. Without a running runner a push or
a tag stays "queued" (no red mark, discarded after 24 h).

- The workflows start only on a push to `main`, a pushed tag, or a finished CI run of such a push. None of them has a
  `pull_request` or `pull_request_target` trigger: a pull request from a fork could change the workflow and run its
  code on the runner's machine. Fork pull requests also need the maintainer's approval before any workflow runs.
- Only Windows is built and tested: there is no Linux or macOS archive and no `install.sh` in a release.
- The first run of a commit that is not signed by a registered device stops at the runner guard of testing.nvim
  (`docs/RUNNER-GUARD.md` there); signed commits of the maintainer pass.
- The sources outside this repository (the engine at the pinned commits of `engine.lock`, appkit through Cargo and
  pnpm) are public and read without credentials. There is no token secret.
- `release-kit` of appkit runs on the runner from v0.23.1 on (Git Bash by its full path); the apps pin it by the
  commit of the tag that `Cargo.lock` pins.
- The repository is public: the install script and the files it fetches are plain downloads, with no login and no
  `GH_TOKEN` (the script still accepts one, see the release-kit README of appkit).

## Not yet

Code signing (Authenticode, notarisation), the installer of a native shell, and the updater key are separate
decisions and arrive with that phase.
