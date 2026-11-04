//! What changed in the state folder, as a hint: the watcher is the kit's, the rule of which paths
//! matter is the testing hub's own and stays here.
//!
//! The results of a repository are `<state>/testing/<project>/{last,last_green,ci}.json`. The state
//! folder is the one of the editor by default, so everything else in it (the shada file, swap files,
//! logs, the other files of testing.nvim, its temp files) must not wake the scan.

use std::path::Path;

pub use appkit_hub::fs_watch::{Entry, Hint, Watcher};
use tokio::sync::mpsc::UnboundedSender;

/// The files of a project that the fleet is read from.
const RESULTS: [&str; 3] = ["last.json", "last_green.json", "ci.json"];

/// Is a change at this path, given relative to the state folder, worth a scan?
///
/// `entry` says what is at the path; it is asked only when the place of the path could matter at all.
#[must_use]
pub fn relevant(relative: &Path, entry: impl FnOnce() -> Entry) -> bool {
    let Some(parts) = relative
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect::<Option<Vec<&str>>>()
    else {
        // A name that is not text: the periodic scan will see what it is.
        return false;
    };
    match parts.as_slice() {
        // The folder of all results, or of one project, that came or went (a folder only: a folder is
        // removed and renamed as a whole, and the system then reports the folder and not the files).
        ["testing"] | ["testing", _] => entry() != Entry::File,
        ["testing", _, file] => RESULTS.contains(file),
        _ => false,
    }
}

/// [`relevant`] as the kit's filter.
fn filter(relative: &Path, entry: &dyn Fn() -> Entry) -> bool {
    relevant(relative, entry)
}

/// Watch the state folder and send a hint for every change that matters.
///
/// # Errors
///
/// The reason, as text: the watcher could not be created or could not watch the folder (one that is
/// not there yet, on a machine that has run no test).
pub fn start(state_dir: &Path, hints: UnboundedSender<Hint>) -> Result<Watcher, String> {
    appkit_hub::fs_watch::start(state_dir, hints, filter).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(p: &str) -> bool {
        relevant(Path::new(p), || Entry::File)
    }

    fn folder(p: &str) -> bool {
        relevant(Path::new(p), || Entry::Folder)
    }

    fn gone(p: &str) -> bool {
        relevant(Path::new(p), || Entry::Gone)
    }

    #[test]
    fn the_results_of_a_project_matter() {
        for yes in [
            "testing/alpha.nvim-0123456789ab/last.json",
            "testing/alpha.nvim-0123456789ab/last_green.json",
            "testing/alpha.nvim-0123456789ab/ci.json",
        ] {
            assert!(file(yes), "{yes}");
            assert!(gone(yes), "{yes}");
        }
    }

    #[test]
    fn a_project_that_came_or_went_matters_and_a_file_of_that_name_does_not() {
        for yes in ["testing", "testing/alpha.nvim-0123456789ab"] {
            assert!(folder(yes), "{yes}");
            assert!(gone(yes), "{yes}");
            assert!(!file(yes), "{yes}");
        }
    }

    #[test]
    fn what_is_not_a_result_does_not_matter() {
        for no in [
            // The rest of the state folder of an editor.
            "shada/main.shada",
            "swap/x.swp",
            "log",
            "last.json",
            "elsewhere/x/last.json",
            // The other files of testing.nvim, and what a write leaves on the way.
            "testing/alpha.nvim-0123456789ab/prev.json",
            "testing/alpha.nvim-0123456789ab/runs.jsonl",
            "testing/alpha.nvim-0123456789ab/last.json.tmp",
            "testing/alpha.nvim-0123456789ab/.last.json.swp",
            "testing/alpha.nvim-0123456789ab/sub/last.json",
        ] {
            assert!(!file(no), "{no}");
        }
    }

    /// `entry` costs a system call: no event outside the results asks it.
    #[test]
    fn a_path_that_cannot_matter_does_not_look_at_the_disk() {
        let never = || -> Entry { panic!("the disk was asked") };
        assert!(!relevant(Path::new("shada/main.shada"), never));
        assert!(!relevant(Path::new("testing/a/prev.json"), never));
        assert!(relevant(Path::new("testing/a/last.json"), never));
    }
}
