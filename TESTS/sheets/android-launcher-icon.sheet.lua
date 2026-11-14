-- The hand check of the launcher icon of testing mobile on a real phone: the icon generator writes the legacy and the
-- adaptive icon into mobile/gen/android (tasks-vault TOOLS/app-icons.md, "Android launcher icon"); only a phone shows
-- how its launcher cuts and tints them. Built and installed by a person; the sheet asks.
--   testing sheets step android-launcher-icon
return {
  id = "testing-desktop.android-launcher-icon",
  title = "testing mobile: the launcher shows the icon of the family, cut to its shape and tinted when themed",
  task = "testing-desktop/abnahme-launcher-icon-von-testing-mobil-auf-dem-fairphone",
  engine = "manual",
  steps = {
    { ask = "The app is built (cargo tauri android build) and installed on the Fairphone over the old one?" },
    {
      ask = "The home screen and the app drawer show the testing icon (green bar, ts), not the green Android robot?",
    },
    {
      ask = "The icon fills the shape of the launcher; the bar, the camera notch and ts are whole, nothing is cut off?",
    },
    {
      ask = "With themed icons on (Wallpaper and style, Themed icons), the icon is tinted"
        .. " and still shows the bar and ts?",
    },
    { ask = "The icon in the list of recent apps and in Settings, Apps, testing is the same?" },
  },
}
