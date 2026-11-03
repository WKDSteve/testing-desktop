//! No test names a folder of its own in the temp dir: the Rust tests get theirs from `Scratch`
//! (`appkit-scratch`), which removes it again, also after a failure; a folder made by hand (a name
//! joined to the temp dir) stays behind whenever a test fails before its own removal.

use std::path::{Path, PathBuf};

fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "fixtures") {
                files(&path, ext, out);
            }
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

/// The lines of the files under `dirs` (relative to the repository) that hold one of `needles`,
/// leaving out the files named in `helpers`.
fn offenders(dirs: &[&str], ext: &str, needles: &[&str], helpers: &[&str]) -> Vec<String> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut all = Vec::new();
    for dir in dirs {
        files(&repo.join(dir), ext, &mut all);
    }
    assert!(!all.is_empty(), "no {ext} file found under {dirs:?}");
    let mut found = Vec::new();
    for file in all {
        if file
            .file_name()
            .is_some_and(|n| helpers.iter().any(|h| n == *h))
        {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for (i, line) in text.lines().enumerate() {
            if needles.iter().any(|n| line.contains(n)) {
                found.push(format!("{}:{}: {}", file.display(), i + 1, line.trim()));
            }
        }
    }
    found
}

#[test]
fn the_rust_tests_get_their_folders_from_scratch() {
    let found = offenders(
        &["crates/testing-hub/src", "crates/testing-hub/tests"],
        "rs",
        &[concat!("temp_dir()", ".join("), concat!("TempDir", "::new")],
        &[],
    );
    assert!(
        found.is_empty(),
        "a folder of its own in the temp dir; use appkit_scratch::Scratch:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_browser_tests_get_their_folders_from_cleanup_ts() {
    let found = offenders(&["ui/e2e"], "ts", &["mkdtemp", "tmpdir()"], &["cleanup.ts"]);
    assert!(
        found.is_empty(),
        "a folder or file of its own in the temp dir; use ui/e2e/cleanup.ts:\n{}",
        found.join("\n")
    );
}
