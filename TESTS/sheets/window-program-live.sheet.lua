-- The hand check of testing-hub as a window program in the real profile: the installed Rolling build, its Start menu
-- link and a login with the autostart. A Claude session may not install or switch the autostart (it sees a redirected
-- AppData and HKCU), so a person does it in a normal terminal. What a machine checks runs elsewhere: testing-hub's tests/cli.rs (--version, --help) and tests/window_program.rs, appkit's
-- tests/window_program.rs (from a terminal the output reaches it, --version, --help and autostart status print there)
-- and the sheet link-start-error (an error of a start from a link is a message box).
--   testing sheets step window-program-live
return {
  id = "testing-desktop.window-program-live",
  feature = "testing-desktop.hub-lifecycle",
  title = "testing-hub: installed, it starts from its link and with the login without a console window",
  task = "testing-desktop/handlauf-hub-als-fenster-programm-im-echten-profil",
  engine = "manual",
  steps = {
    {
      ask = "In a normal terminal (not a Claude session): testing-desktop from the Rolling build installed (install.ps1 of"
        .. " the README), then started from the Start menu link: no console window comes up and the page opens?",
    },
    { ask = "A second click on the link only opens the page again (still one testing-hub in the Task Manager)?" },
    {
      ask = "testing-hub autostart on, signed out and in again: no console window at the login, and the hub runs"
        .. " (testing-hub open finds it)?",
    },
    {
      ask = "testing-hub stop ends the hub: afterwards no testing-hub.exe and no nvim.exe of the hub is left in the Task"
        .. " Manager?",
    },
    {
      ask = "Optional, the real case of the message box: with port 17618 taken by another program, the start from"
        .. " the link shows a message box that names the cause?",
    },
  },
}
