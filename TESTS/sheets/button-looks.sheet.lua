-- The looks of the filled button of the web variant's way in ("Choose folder", in the single file the build makes): at
-- rest, under the pointer and with the keyboard focus, light and dark. The colours themselves are checked by
-- ui/e2e-web/page.spec.ts; this sheet leaves the shots for the judgement whether one sees the difference. The steps
-- find the button by its place, not its text: the page speaks the browser's language.
--   testing sheets step button-looks --headed
return {
  id = "testing-desktop.button-looks",
  title = "testing-desktop: the filled button of the web way in at rest, under the pointer and with focus, light and dark",
  task = "testing-desktop/live-check-gefuellter-knopf-des-web-einstiegs-hell-und-dunke",
  engine = "browser",
  app = "web",
  sizes = { { cols = 1000, rows = 700 } },
  steps = {
    { scheme = "light" },
    { open = "${link}", note = "the single file, built for this run" },
    { wait = "document.querySelector('.entry button.primary') !== null", timeout = 10000, note = "the way in asks for a folder" },
    { hover = "#web-entry-title", note = "the pointer on the heading" },
    { expect = "!document.querySelector('.entry button.primary').matches(':hover')" },
    { shot = "choose-rest-light" },
    { hover = ".entry button.primary" },
    { expect = "document.querySelector('.entry button.primary').matches(':hover')" },
    { shot = "choose-hover-light" },
    { hover = "#web-entry-title" },
    { focus = ".entry button.primary" },
    { expect = "document.querySelector('.entry button.primary').matches(':focus-visible')", note = "the focus ring shows" },
    { shot = "choose-focus-light" },
    { expect = "(document.activeElement.blur(), document.activeElement === document.body)", note = "the focus off again" },
    { scheme = "dark" },
    { expect = "matchMedia('(prefers-color-scheme: dark)').matches", note = "the page sees the dark scheme" },
    { hover = "#web-entry-title" },
    { expect = "!document.querySelector('.entry button.primary').matches(':hover')" },
    { shot = "choose-rest-dark" },
    { hover = ".entry button.primary" },
    { expect = "document.querySelector('.entry button.primary').matches(':hover')" },
    { shot = "choose-hover-dark" },
    { hover = "#web-entry-title" },
    { focus = ".entry button.primary" },
    { expect = "document.querySelector('.entry button.primary').matches(':focus-visible')", note = "the focus ring shows" },
    { shot = "choose-focus-dark" },
  },
  judge = {
    "Under the pointer the filled button Choose folder is clearly lighter or darker than at rest, light and dark",
    "With the focus a ring around it is plainly visible, light and dark",
    "Its text stays readable in every state",
  },
}
