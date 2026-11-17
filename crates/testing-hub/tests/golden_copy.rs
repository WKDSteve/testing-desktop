//! The golden documents of `tests/fixtures/golden` and the types of the page (`ui/src/lib/contract.d.ts`) are
//! copies of testing.nvim's at the commit of `engine.lock`: a bump of the pin that changes the contract shows here.
//! The files are read from that commit (`git show`), whatever the checkout has out. The checkout is
//! `TESTING_ENGINE_DIR`, else `$REPOS_DIR/testing.nvim`; without one the test says so and passes, unless
//! `TESTING_HUB_REQUIRE_ENGINE=1` (CI).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Each copy (relative to this repository) and the file of testing.nvim it is a copy of.
const COPIES: [(&str, &str); 13] = [
    (
        "crates/testing-hub/tests/fixtures/golden/findings.json",
        "TESTS/golden/testing-export-1-findings/findings.json",
    ),
    (
        "crates/testing-hub/tests/fixtures/golden/fleet.json",
        "TESTS/golden/testing-export-1/fleet.json",
    ),
    (
        "crates/testing-hub/tests/fixtures/golden/repo.json",
        "TESTS/golden/testing-export-1/repos/beta.nvim-ac6d3a4ece7f.json",
    ),
    (
        "crates/testing-hub/tests/fixtures/golden/hello.json",
        "TESTS/golden/testing-export-1-hello/hello.json",
    ),
    (
        "ui/src/lib/contract.d.ts",
        "schemas/testing-export/1/testing-export.d.ts",
    ),
    // The schema the check of the web page is compiled from (ui/web/gen-validate.mjs).
    (
        "ui/src/web/testing-export.schema.json",
        "schemas/testing-export/1/testing-export.schema.json",
    ),
    // The schema the check of a recording's lines is compiled from (the replay of a stepped run in the web page).
    (
        "ui/src/web/testing-sheet-session.schema.json",
        "schemas/testing-sheet-session/1/testing-sheet-session.schema.json",
    ),
    // The document of a run of sheets the hub reads (`launch::sheet_result`): what its fields mean is said there.
    (
        "crates/testing-hub/tests/fixtures/testing-sheet-run.schema.json",
        "schemas/testing-sheet-run/1/testing-sheet-run.schema.json",
    ),
    // A whole export folder: what the web page reads, with its manifest and the sums in it.
    (
        "crates/testing-hub/tests/fixtures/golden-export/testing-export.json",
        "TESTS/golden/testing-export-1/testing-export.json",
    ),
    (
        "crates/testing-hub/tests/fixtures/golden-export/fleet.json",
        "TESTS/golden/testing-export-1/fleet.json",
    ),
    (
        "crates/testing-hub/tests/fixtures/golden-export/repos/alpha.nvim-356bf237c7fc.json",
        "TESTS/golden/testing-export-1/repos/alpha.nvim-356bf237c7fc.json",
    ),
    (
        "crates/testing-hub/tests/fixtures/golden-export/repos/beta.nvim-ac6d3a4ece7f.json",
        "TESTS/golden/testing-export-1/repos/beta.nvim-ac6d3a4ece7f.json",
    ),
    (
        "crates/testing-hub/tests/fixtures/golden-export/repos/gamma.nvim-bed2b5150df9.json",
        "TESTS/golden/testing-export-1/repos/gamma.nvim-bed2b5150df9.json",
    ),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The `commit` of `engine.lock`.
fn pinned() -> String {
    let text = std::fs::read_to_string(repo_root().join("engine.lock")).unwrap();
    let commit = text
        .lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == "commit")
        .map(|(_, v)| v.trim().to_owned())
        .expect("engine.lock names a commit");
    assert!(
        commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "{commit}"
    );
    commit
}

fn checkout() -> Option<PathBuf> {
    std::env::var_os("TESTING_ENGINE_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("REPOS_DIR").map(|r| PathBuf::from(r).join("testing.nvim")))
        .filter(|d| d.join("scripts").join("testing.lua").is_file())
}

#[test]
fn the_golden_copies_are_the_files_of_the_pinned_engine() {
    let Some(dir) = checkout() else {
        assert!(
            std::env::var_os("TESTING_HUB_REQUIRE_ENGINE").is_none_or(|v| v != "1"),
            "TESTING_HUB_REQUIRE_ENGINE=1 and no testing.nvim checkout"
        );
        eprintln!("skipped: no testing.nvim checkout");
        return;
    };
    let commit = pinned();
    for (copy, original) in COPIES {
        let out = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["show", &format!("{commit}:{original}")])
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "{original} at {commit}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let ours = std::fs::read(repo_root().join(copy)).unwrap();
        // git may hand a text file over with CRLF on Windows (core.autocrlf); the contract is LF.
        let theirs: Vec<u8> = out.stdout.into_iter().filter(|b| *b != b'\r').collect();
        assert_eq!(ours, theirs, "{copy} is not {original} at {commit}");
    }
}

/// Every checkout `engine.lock` pins (testing.nvim, lib.nvim, tasks.nvim) is a whole commit, and the CI fetches each
/// of them in every job that fetches the engine: a pin added to the file but not to `ci.yml` would leave a test of
/// that checkout skipped or failing for a reason that is not its own.
#[test]
fn every_pin_of_engine_lock_is_a_whole_commit_that_the_ci_fetches() {
    let text = std::fs::read_to_string(repo_root().join("engine.lock")).unwrap();
    let pins: Vec<(String, String)> = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let commits: Vec<&(String, String)> =
        pins.iter().filter(|(k, _)| k.ends_with("commit")).collect();
    let names: Vec<&str> = commits.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, ["commit", "lib_commit", "tasks_commit"], "{text}");
    let ci = std::fs::read_to_string(repo_root().join(".github/workflows/ci.yml")).unwrap();
    let fetches = ci.matches("Fetch the pinned engine").count();
    assert!(fetches >= 2, "{fetches}");
    for (key, commit) in commits {
        assert!(
            commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
            "{key} = {commit}"
        );
        let repo_key = key.replace("commit", "repo");
        let repo = &pins
            .iter()
            .find(|(k, _)| *k == repo_key)
            .unwrap_or_else(|| panic!("engine.lock names {repo_key}"))
            .1;
        let read = format!("{key}=\"$(sed -n 's/^{key} *= *//p' engine.lock)\"");
        let name = repo.rsplit('/').next().unwrap();
        let clone = format!("git clone --quiet {repo} engine/{name}");
        let checkout = format!("git -C engine/{name} checkout --quiet \"${key}\"");
        for (what, needle) in [("read", &read), ("clone", &clone), ("checkout", &checkout)] {
            assert_eq!(
                ci.matches(needle.as_str()).count(),
                fetches,
                "ci.yml does not {what} {key} in every fetch: {needle}"
            );
        }
    }
}
