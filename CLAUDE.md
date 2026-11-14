# testing-desktop (Rust-Hub + Svelte-Oberfläche zu testing.nvim)

CI, Rolling-Bau und Release laufen auf dem selbst gehosteten Windows-Runner (nur Windows, kein
`pull_request`-Trigger; siehe `docs/RELEASING.md`). Hub und Oberfläche werten keine Ergebnisse selbst aus und starten
nichts Eigenes: alles kommt aus der Engine (`testing.nvim`, Verträge `testing-export/1`, `testing-sheet-session/1`,
`testing-ci-release/1`), die Seite zeigt nur. Was der Hub startet, ist die Kommandozeile der Engine mit festem argv
(`docs/SECURITY.md`, Abschnitt "What is the testing hub's own").

## Bauen und prüfen (vor "fertig")

Dieselben Schritte wie `.github/workflows/ci.yml`, in dieser Reihenfolge (die Seite vor dem Binary: es bettet
`ui/dist` ein, `ui/e2e/hub.ts` lehnt ein Binary mit älterer Seite ab):

```
cd ui && pnpm install --frozen-lockfile && pnpm check && pnpm test && pnpm build && pnpm mobile:build && cd ..
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python -m unittest discover -s tools -p "test_*.py"
python tools/check-control-chars.py
python tools/check-color-literals.py --tokens ui/src/palette.css
python tools/check-lock-age.py --min-days 7     # fragt crates.io; beim ersten Lauf eines frischen Worktrees Minuten
python tools/check-appkit-pin.py
cargo build --locked
cd ui && pnpm e2e                                # echtes Binary, echte Engine, axe hell und dunkel
cd ui && pnpm web:build && pnpm web:e2e          # testing.html auf file://, keine Anfrage, keine CSP-Verletzung
```

Dazu die Testblätter wie der CI-Job `ui` (aus dem Repo-Root, `--strict`):
`nvim -n -i NONE --headless -u NONE -l <testing.nvim>/scripts/testing.lua sheets run --all --root . --engine browser --strict`
mit `TESTING_TRUST=1` und `LIB_NVIM_DIR`.

- Umgebung für Engine- und Browser-Tests: `TESTING_ENGINE_DIR`, `LIB_NVIM_DIR`, `TESTING_HUB_REQUIRE_ENGINE=1`. Ohne
  die letzte werden Tests mit Engine **still übersprungen** und die Suite ist grün, ohne etwas geprüft zu haben.
- Gegen den **gepinnten** Stand testen, nicht gegen den Haupt-Checkout der Engine (der hinkt `origin` oft hinterher
  oder trägt lokale Commits anderer Sitzungen): eigener Git-Worktree je Repo auf dem Commit aus `engine.lock`
  (`git -C E:/repos/testing.nvim worktree add --detach <pfad> <commit>`, ebenso lib.nvim mit `lib_commit`).
- Ein frischer Worktree braucht ein eigenes `pnpm install` und einen eigenen `cargo build`; den `target`-Ordner des
  Haupt-Checkouts nie mitbenutzen.
- Ein Fix ohne Test, der ohne den Fix rot wird, gilt nicht als fertig (Sabotage-Probe: Fix kurz entfernen).
- Die Maschine ist der Engpass (mehrere Sitzungen, die Runner laufen auf demselben Rechner): schwere Läufe
  (`cargo test`, `pnpm e2e`) nacheinander und im Hintergrund. Unter Last flackern einzelne Tests: erst einzeln
  wiederholen und den Trace lesen (`ui/test-results*/`), dann ändern.
- Der Hosted-CI-Lauf eines Pushs auf main erscheint bei GitHub manchmal erst zehn Minuten später. Vor einem
  Handstart (`workflow_dispatch`) so lange warten, sonst läuft die CI doppelt.

## appkit (der geteilte Unterbau)

- Wächter, Pairing, Engine-Naht, Sicherheitsmatrix, Seiten-Bausteine (`appkit-ui`), Web-Variante (`appkit-web`) und
  Release-Kit kommen aus [appkit](https://github.com/WKDSteve/appkit) (`E:/repos/appkit`), gepinnt per Tag an allen
  Stellen: `Cargo.toml` (`[workspace.dependencies]`), `ui/package.json` (`github:WKDSteve/appkit#vX.Y.Z&path:ui-kit`
  bzw. `web-kit`) und die `uses: WKDSteve/appkit/release-kit/...@<sha>` in `rolling.yml` und `release.yml`.
  `python tools/check-appkit-pin.py` prüft, dass alle auf denselben Commit zeigen.
- Eine Änderung am Kit geht erst durch appkit (CI grün, neues Tag), dann hier das Tag heben. Für alle Apps auf einmal:
  `python tools/bump-apps.py vX.Y.Z` in appkit.
- Gleichzeitig an appkit und hier arbeiten: `[patch."https://github.com/WKDSteve/appkit"]` nur in einer **nicht
  eingecheckten** `.cargo/config.toml`. Ein `[patch]` schreibt `Cargo.lock` um: nie so committen.

## Vertrag mit der Engine

- `engine.lock` pinnt testing.nvim und lib.nvim per Commit, und nur auf einen Commit, der auf `origin/main` liegt
  (nie `rev-parse HEAD` eines lokalen Checkouts: die CI scheitert mit "not our ref").
- Kopien der Engine liegen hier byte-genau und werden in `crates/testing-hub/tests/golden_copy.rs` gegen den
  gepinnten Commit verglichen: die Goldens unter `crates/testing-hub/tests/fixtures/golden*`, `ui/src/lib/contract.d.ts`,
  `ui/src/web/testing-export.schema.json` und `ui/src/web/testing-sheet-session.schema.json`. Nach einem Sprung des
  Pins alle aus dem neuen Commit holen (`git show <commit>:<pfad>`, LF) und `pnpm web:gen` laufen lassen: die
  Prüfungen der Web-Variante sind vorab kompilierter Ajv-Code (`validate*.generated.js`, die CSP erlaubt kein eval);
  `ui/web/web.test.mjs` fällt rot, wenn sie nicht zum Schema passen.
- Ändert sich ein Vertrag oder fehlt der Engine ein Schritt: **Engine zuerst** (eigener Worktree, Spec, Doku,
  `testing ci land`, siehe `CLAUDE.md` von testing.nvim), dann hier den Pin heben (eigener `chore(engine)`-Commit).
- Ein Vertrag lässt Felder zu, die die Engine schreibt, aber eine Aufzeichnung weglässt (`root` in `start` von
  `events.jsonl`): die Prüfung setzt solche Felder vor dem Prüfen ein, statt das Schema zu lockern.

## Sicherheit (was nicht verhandelbar ist)

- Jede Route steht in `ROUTES` (`crates/testing-hub/src/routes.rs`); nur so probiert die Sicherheitsmatrix sie ohne
  Sitzung. Keine Route daneben.
- Was ein Telefon erreicht, steht allein in `phone::DEVICE_ROUTES`; die Tests in `phone.rs` halten fest, dass kein
  Telefon einen Schrittlauf, einen Lauf einer Suite, den Freigabe-Assistenten oder den Event-Stream erreicht, und dass
  keine Route des Hubs einer Wurzel vertraut oder einen Befehl einer Anfrage ausführt. Der Freigabe-Assistent für die
  PC-Sitzung ist so gewollt (entschieden 2026-11-14).
- Der Hub baut nichts der Engine nach: keine eigene Liste geprüfter Commits, keine eigene GitHub-Abfrage zu einem
  Commit, keine eigene KI-Anfrage. Kein Schlüssel eines Modells im Hub; gesendet wird eine KI-Anfrage nur über den
  Headless-Einstieg von ai.nvim, und erst nachdem die Seite Anfrage und Ziel gezeigt hat (PRIN-60).
- Eine echte Freigabe (`testing ci release ... --yes`, `POST /api/testing/release`) löst eine Sitzung nie selbst aus:
  sie ändert die Liste dieses Rechners. Getestet wird gegen `tests/fixtures/fake-release.lua`.
- Das Kit bricht jede Anfrage nach 30 s ab (`Limits::request_timeout`): was länger dauern kann (eine Freigabe), läuft
  im Hintergrund und wird über eine eigene Route abgefragt.

## Browser-Tests (Playwright)

- Die Hub-Tests (`pnpm e2e`) nehmen je Lauf einen freien Port (`TESTING_E2E_PORT` erzwingt einen) und einen eigenen
  Vault unter dem Temp-Ordner; `TESTING_RUNNER_ROOT` zeigt dort auf einen leeren Ordner, nie auf die echte Liste.
- Die Schritte des Freigabe-Assistenten, die GitHub brauchen, sind in `ui/e2e/release.spec.ts` per `page.route`
  nachgestellt; der erste Schritt geht durch die echte Engine.
- Die Zwischenablage darf die Seite schreiben, aber im Test nicht lesen (Permissions-Policy): `writeText` per
  Init-Skript mitschneiden statt `readText`.
- `getByText('Screen')` und ähnliche kurze Wörter treffen auch Erklärtexte: `{ exact: true }` nehmen.
- testing-web: `ui/e2e-web/page.ts` baut den Ordner nach (`fakePicker`), Binärdateien als `base64:`-Text (`binary()`).
  Jeder Test prüft, dass die Seite keine Anfrage stellt und die CSP nicht verletzt (`clean`).

## Nie

- Den Hub nicht aus einer Claude-Sitzung installieren oder starten (`testing-hub install|start|autostart`): die
  Claude-App leitet AppData und HKCU um.
- Keinen Workflow anstoßen, der etwas veröffentlicht (`web.yml`, `release.yml`, `rolling.yml`).
- Keine Pull Requests: fertig, committen, pullen, auf `main` pushen (`git push origin HEAD:main`). Vor dem Push
  `git log origin/main..HEAD` ansehen. Keine Co-Autorenschaft von Claude, Commits signiert.
