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
pub async fn install(opts: InstallOptions) -> Result<String, String> {
    let text = appkit_hub::background::install(&APP, opts).await?;
    let env = appkit_hub::env::Env::from_process();
    let note = appkit_hub::background::installed_program(&APP, &env)
        .map(|target| how_to_start(&target, std::env::var_os("PATH").as_deref()))
        .unwrap_or_default();
    Ok(format!("{text}{note}"))
}

/// The last lines of `install`: the whole call that starts the installed program, and whether its
/// folder is on `PATH` (install does not put it there: a program of the user is not added to the
/// path of every shell behind their back).
#[must_use]
pub fn how_to_start(target: &std::path::Path, path_var: Option<&std::ffi::OsStr>) -> String {
    let dir = target.parent().unwrap_or(target);
    let same = |p: &std::path::Path| {
        if cfg!(windows) {
            p.to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .eq_ignore_ascii_case(dir.to_string_lossy().trim_end_matches(['\\', '/']))
        } else {
            p == dir
        }
    };
    let on_path = path_var.is_some_and(|v| std::env::split_paths(v).any(|p| same(&p)));
    let mut out = format!("start it: \"{}\" start --open\n", target.display());
    if on_path {
        out.push_str("its folder is on PATH: `testing-hub start --open` works in any shell\n");
    } else {
        out.push_str(&format!(
            "its folder is not on PATH; add {} to call `testing-hub` by name{}\n",
            dir.display(),
            if cfg!(windows) {
                " (the Start menu entry Testing starts it too)"
            } else {
                ""
            }
        ));
    }
    out
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

    /// `install` ends with the call that starts the program, and says whether its folder is on PATH.
    #[test]
    fn install_names_the_whole_call_and_the_path() {
        // Whole paths of no folder that exists: only compared, nothing is made.
        let root = std::path::PathBuf::from(if cfg!(windows) {
            "C:/testing-hub-test"
        } else {
            "/testing-hub-test"
        });
        let dir = root.join("bin");
        let target = dir.join(crate::IDENTITY.program_file());
        let elsewhere = std::env::join_paths([root.join("elsewhere")]).unwrap();
        let off = how_to_start(&target, Some(&elsewhere));
        assert!(
            off.starts_with(&format!(
                "start it: \"{}\" start --open\n",
                target.display()
            )),
            "{off}"
        );
        assert!(off.contains("not on PATH"), "{off}");
        assert!(off.contains(&dir.display().to_string()), "{off}");
        let none = how_to_start(&target, None);
        assert!(none.contains("not on PATH"), "{none}");
        let with = std::env::join_paths([root.join("elsewhere"), dir.clone()]).unwrap();
        let on = how_to_start(&target, Some(&with));
        assert!(on.contains("is on PATH"), "{on}");
        assert!(!on.contains("not on PATH"), "{on}");
        if cfg!(windows) {
            // Windows does not tell the case of a folder apart, nor a trailing separator.
            let upper =
                std::ffi::OsString::from(format!("{}\\", dir.display().to_string().to_uppercase()));
            assert!(how_to_start(&target, Some(&upper)).contains("is on PATH"));
        }
    }

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
