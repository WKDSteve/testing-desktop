//! The hub as a background program (`start`, `stop`, `autostart`, `install`, `uninstall`): the work
//! is the kit's, this module states what is the testing hub's own.

use std::future::Future;
use std::pin::Pin;

use appkit_hub::background::Background;
pub use appkit_hub::background::{Autostart, InstallOptions, UninstallOptions};

/// The files of the testing hub in the config folder besides those every hub has: none.
const EXTRA_FILES: [&str; 0] = [];

/// What the background commands know about this app.
pub(crate) const APP: Background<'static> = Background {
    login: crate::LOGIN,
    extra_files: &EXTRA_FILES,
    uninstall_script: "scripts/uninstall.ps1 or scripts/uninstall.sh",
};

/// Start a hub in the background unless one runs. Returns what to print.
///
/// # Errors
///
/// A text for the person at the terminal.
pub async fn start() -> Result<String, String> {
    appkit_hub::background::start(&APP).await
}

/// Stop the hub that runs (the one `hub.json` names and that answers as it).
///
/// # Errors
///
/// A text for the person at the terminal.
pub async fn stop() -> Result<String, String> {
    appkit_hub::background::stop(&APP).await
}

/// Start the hub with the login of the user.
///
/// # Errors
///
/// A text for the person at the terminal.
pub fn autostart(what: Autostart) -> Result<String, String> {
    appkit_hub::background::autostart(&APP, what)
}

/// Copy this program to a fixed place and start the hub from there again when one was running.
///
/// # Errors
///
/// A text for the person at the terminal.
/// The kit ends with the whole call that starts the program and whether its folder is on PATH
/// (`appkit_hub::background::how_to_start`).
pub async fn install(opts: InstallOptions) -> Result<String, String> {
    appkit_hub::background::install(&APP, opts).await
}

/// Take the integration of the program with the system away.
pub fn uninstall(
    opts: UninstallOptions,
) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> {
    appkit_hub::background::uninstall(&APP, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `uninstall --purge-settings` removes the kit's files and nothing else.
    #[test]
    fn the_testing_hub_has_no_files_besides_the_kits() {
        assert!(APP.extra_files.is_empty());
        assert_eq!(APP.login.summary, "the test and CI results");
        assert!(crate::IDENTITY.program_file().starts_with("testing-hub"));
    }

    /// The desktop link: the title of the app, `start --open`, minimized, the description of the app.
    #[test]
    fn the_desktop_link_starts_the_hub_and_opens_the_page() {
        assert_eq!(
            appkit_hub::background::desktop_link_name(crate::IDENTITY.title),
            "Testing.lnk"
        );
        let link = appkit_hub::background::app_link(
            crate::IDENTITY.app,
            std::path::Path::new(r"C:\x\testing-hub.exe"),
            appkit_hub::background::LinkStart::OpenPage,
        );
        assert_eq!(link.target, r"C:\x\testing-hub.exe");
        assert_eq!(link.arguments, "start --open");
        assert_eq!(link.working_dir, r"C:\x");
        assert_eq!(link.show_command, 7);
        assert_eq!(
            link.description,
            "testing-desktop: start the hub and open the page"
        );
    }
}
