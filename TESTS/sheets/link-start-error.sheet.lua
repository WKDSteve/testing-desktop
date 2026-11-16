-- A start from the link that fails: the release build of testing-hub is a window program, so nobody reads its stderr
-- there and the error must come as a message box (appkit console::report). The app `link-start-error` of .testing.lua
-- starts it as a link does, against a config.json it cannot use; the message box is its only window. Without the start
-- as by a link (stderr read by the run) the program shows no window and the sheet is red. What stays for a person is
-- in the manual sheet window-program-live (the real Start menu link and login).
--   testing sheets run --sheet link-start-error
return {
  id = "testing-desktop.link-start-error",
  feature = "testing-desktop.hub-lifecycle",
  title = "testing-hub: a start from a link that fails says so in a message box",
  task = "testing-desktop/handlauf-hub-als-fenster-programm-im-echten-profil",
  engine = "desktop",
  app = "link-start-error",
  sizes = { { cols = 640, rows = 240 } },
  steps = {
    { wait = "OK", timeout = 5000, note = "the message box of testing-hub is up, with its button" },
    { shot = "message" },
    { click = "OK", note = "the person closes it" },
  },
  judge = { "The message says why the hub did not start and where to read more (hub.log)" },
}
