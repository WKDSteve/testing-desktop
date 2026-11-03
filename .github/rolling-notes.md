**This is not a release.** It is the newest state of `main`, rebuilt after every green CI run and replaced each time. Nothing here is signed or tested by hand; use a release for anything that matters. The program shows the test and CI results of your repositories that use testing.nvim, and only reads them: it starts no test run and writes into no repository.

**Windows only** (x86-64). The files of this page are plain downloads: no GitHub login and no token.

**Install it with the script** (for the current user, no administrator rights, and no SmartScreen stop: the script downloads the program itself and checks it against `SHA256SUMS`). In PowerShell (not elevated):

```
irm https://github.com/WKDSteve/testing-desktop/releases/download/latest/install.ps1 -OutFile install.ps1
.\install.ps1
```

Run it again to update to the newest rolling build. If you save `install.ps1` from this page with the browser instead, run `Unblock-File .\install.ps1` first: the browser marks the file as downloaded, and PowerShell refuses such an unsigned script ("is not digitally signed").

Or take the file yourself: `testing-hub-windows-x86_64.exe`. A double click starts the hub in the background and opens the browser (a second click finds the running hub). Windows SmartScreen warns about an unsigned program: "More info", then "Run anyway". `uninstall.ps1` removes it again; `SHA256SUMS` holds the checksums of all files.

The program needs **Neovim 0.12 or newer** and checkouts of `testing.nvim` and `lib.nvim` (the engine; found through `TESTING_ENGINE_DIR` and `LIB_NVIM_DIR`, `REPOS_DIR`, or the settings file). It shows only the repositories that the `areas.json` of your device releases. Without them the page says what is missing. `testing-hub --version` names the commit this file was built from; `testing-hub stop` ends the hub. See `docs/USAGE.md`.
