//! The golden documents of `tests/fixtures/golden` are copies of testing.nvim's at the commit of
//! `engine.lock`: a bump of the pin that changes the contract shows here. The files are read from
//! that commit (`git show`), whatever the checkout has out. The checkout is `TESTING_ENGINE_DIR`,
//! else `$REPOS_DIR/testing.nvim`; without one the test says so and passes, unless
//! `TESTING_HUB_REQUIRE_ENGINE=1` (CI).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Each copy and the file of testing.nvim it is a copy of.
const COPIES: [(&str, &str); 3] = [
    ("fleet.json", "TESTS/golden/testing-export-1/fleet.json"),
    (
        "repo.json",
        "TESTS/golden/testing-export-1/repos/beta.nvim-ac6d3a4ece7f.json",
    ),
    (
        "hello.json",
        "TESTS/golden/testing-export-1-hello/hello.json",
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
        let ours = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("golden")
                .join(copy),
        )
        .unwrap();
        // git may hand a text file over with CRLF on Windows (core.autocrlf); the contract is LF.
        let theirs: Vec<u8> = out.stdout.into_iter().filter(|b| *b != b'\r').collect();
        assert_eq!(ours, theirs, "{copy} is not {original} at {commit}");
    }
}
