-- TESTS/sheets/features.lua -- the functionalities of testing-desktop as its user meets them; `testing sheets report`
-- says which ones have a feature sheet (`feature = "<id>"`) and in which state, and K16 warns about a sheet that names
-- an id not listed here. Data: read with an empty environment like a sheet (testing.nvim docs/SHEETS.md).
-- The list follows README.md, docs/USAGE.md and docs/MOBILE.md. testing-desktop has no command surface for
-- `testing surface`, so no entry names one.
return {
  {
    id = "testing-desktop.install",
    title = "Install, start with Windows, a shortcut, and uninstall (testing-hub install / autostart / uninstall)",
    docs = "docs/USAGE.md",
  },
  {
    id = "testing-desktop.hub-lifecycle",
    title = "Start the hub in the background, stop it, and open the page through a one-time pairing link",
    docs = "docs/USAGE.md",
  },
  {
    id = "testing-desktop.fleet",
    title = "The fleet: every repository with its last local run and the CI per system, red first, filter and search",
    docs = "docs/USAGE.md",
  },
  {
    id = "testing-desktop.live-results",
    title = "A run in a terminal or a new CI record shows on the open page by itself, without a reload",
    docs = "docs/USAGE.md",
  },
  {
    id = "testing-desktop.repository",
    title = "One repository: its red cases with their output and its findings, a cell's word explained on hover",
    docs = "docs/USAGE.md",
  },
  {
    id = "testing-desktop.history",
    title = "The history of a repository's runs: searchable, cut to a period, saved as CSV or JSON",
    docs = "README.md",
  },
  {
    id = "testing-desktop.cost",
    title = "What the heavy runs cost on this machine, per repository and for the whole fleet",
    docs = "README.md",
  },
  {
    id = "testing-desktop.sheet-step",
    title = "Step through a test sheet like a debugger: next, continue, until here, breakpoints, notes, questions",
    docs = "docs/USAGE.md",
  },
  {
    id = "testing-desktop.web-variant",
    title = "testing.html opened from disk shows a folder of testing export, with no server and no request",
    docs = "docs/USAGE.md",
  },
  {
    id = "testing-desktop.button-states",
    title = "The filled button of the web way in shows rest, pointer and keyboard focus apart, light and dark",
    docs = "docs/USAGE.md",
  },
  {
    id = "testing-desktop.hub-tile",
    title = "The tile Testing in hub-desktop starts the hub and opens it with one click",
    docs = "docs/USAGE.md",
  },
  {
    id = "testing-desktop.phone-pairing",
    title = "Pair testing mobil by a QR code, list the phones, set their level or revoke one",
    docs = "docs/MOBILE.md",
  },
  {
    id = "testing-desktop.phone-app",
    title = "testing mobil: the fleet and red cases on the phone, a notice when a main turns red, rerun failed jobs",
    docs = "docs/MOBILE.md",
  },
}
