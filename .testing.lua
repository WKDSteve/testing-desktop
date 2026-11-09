-- .testing.lua -- what the test sheets of this repository run against (testing.nvim, docs/SHEETS.md there). The sheets
-- in TESTS/sheets only name an app; how it starts is here, under testing.nvim's trust gate. testing-desktop has no Lua
-- spec.

return {
  sheets = {
    -- the Chrome of the machine, as the e2e tests; playwright-core comes at the version of ui/pnpm-lock.yaml
    browser = { playwright_dir = "ui", channel = "chrome" },
    apps = {
      -- The web variant, the single file ui/dist-web/testing.html, built fresh for the run (no server, no hub). The
      -- "app" only says where the file is, as a file: link, and waits until the sheet is done.
      ["web"] = {
        build = { "pnpm", "--dir", "ui", "web:build" },
        run = {
          "node",
          "-e",
          "console.log(require('url').pathToFileURL(require('path').resolve('ui/dist-web/testing.html')).href);"
            .. "process.stdin.resume()",
        },
        ready = { stdout = "^file:", as = "link" },
      },
    },
  },
}
