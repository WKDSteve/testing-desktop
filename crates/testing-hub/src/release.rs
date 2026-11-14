//! The release assistant for the runner guard (`release/*` routes, `releasing`): the steps of testing.nvim's own
//! `testing ci release`, asked one by one as documents of `testing-ci-release/1`. A release lets the code of a commit
//! onto the self-hosted runner of this machine, so nothing of it is the hub's own:
//!
//! * the hub starts the engine's command line as a child and reads its one document; it never reads or writes the
//!   list of reviewed commits itself, never asks GitHub about a commit itself, and never decides what is released:
//!   `testing ci release <owner/repo> <sha> --yes --json` does, through `testing ci trust`, as in a terminal;
//! * the argv is fixed: `ci release`, a word of a closed list (`--repos`, `--commits`, `--show`, `--yes`), `--json`,
//!   and at most a repository (`owner/name` of plain characters) and a commit id (40 hex digits): never a path, a
//!   note, a count or another option of a request;
//! * the release names the full commit id, and only the one whose review aid (`--show`) the hub handed out last, not
//!   long ago ([`SHOWN_FOR`]): "the head of main" can move, the commit a person looked at cannot. The aid is spent by
//!   the release it allowed;
//! * one step at a time, each under a time limit; the routes are for a session of this PC only (no phone reaches
//!   them, `phone::DEVICE_ROUTES`);
//! * a reading step is answered within the time the hub gives a request ([`READ_LIMIT`]). The release is not tied to
//!   a request: it is started, runs to its end whatever becomes of the page that asked, and its outcome is kept to be
//!   read ([`Release::last`]). A request that was cut off must never end a release half way.
//!
//! `gh` is asked by the engine, with the person's own login of `gh` (the hub hands it no token).

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::io::AsyncReadExt;

use crate::rerun::plain_repo;
use crate::sheetstep::{hide_root, StepProgram};

/// The contract of the documents the engine prints.
pub const SCHEMA: &str = "testing-ci-release/1";

/// How long a reading step may take: less than the hub gives a request (`Limits::request_timeout`, 30 seconds), so
/// that a slow GitHub is this hub's own answer and not a request cut off.
pub const READ_LIMIT: Duration = Duration::from_secs(25);

/// How long the release may take: the entry in the list, then one `gh run rerun` per failed run of the commit.
pub const RELEASE_LIMIT: Duration = Duration::from_secs(10 * 60);

/// How long a review aid allows the release of its commit.
pub const SHOWN_FOR: Duration = Duration::from_secs(15 * 60);

/// The largest document of a step: a review aid lists at most 300 files and 250 commits.
const MAX_OUTPUT: usize = 4 * 1024 * 1024;

/// The variables of the person's environment the steps read beyond what a stepped run gets: where the list of
/// reviewed commits is kept (the runner guard reads the same), and where `gh` keeps its login.
pub const PASS_THROUGH: &[&str] = &["TESTING_RUNNER_ROOT", "GH_CONFIG_DIR", "XDG_CONFIG_HOME"];

/// One step of the assistant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The repositories on the list of reviewed commits.
    Repos,
    /// The last commits of the default branch of a repository, each with whether it is released.
    Commits { repo: String },
    /// The review aid: what changed since the last reviewed commit, with the files that deserve a close look.
    Show { repo: String, sha: String },
    /// The release of that commit and the re-run of its failed runs.
    Release { repo: String, sha: String },
}

impl Step {
    /// The word of the step, as the engine's document names it.
    #[must_use]
    pub fn word(&self) -> &'static str {
        match self {
            Self::Repos => "repos",
            Self::Commits { .. } => "commits",
            Self::Show { .. } => "show",
            Self::Release { .. } => "release",
        }
    }

    /// The arguments after the engine's script. The repository and the commit id are checked by [`is_repo`] and
    /// [`is_full_sha`] before a step is made of them.
    #[must_use]
    pub fn args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec!["ci".into(), "release".into()];
        match self {
            Self::Repos => args.push("--repos".into()),
            Self::Commits { repo } => args.extend([repo.into(), "--commits".into()]),
            Self::Show { repo, sha } => args.extend([repo.into(), sha.into(), "--show".into()]),
            Self::Release { repo, sha } => args.extend([repo.into(), sha.into(), "--yes".into()]),
        }
        args.push("--json".into());
        args
    }

    fn limit(&self) -> Duration {
        match self {
            Self::Release { .. } => RELEASE_LIMIT,
            _ => READ_LIMIT,
        }
    }
}

/// Is this a repository as a request may name it: `owner/name` of the characters GitHub allows in both.
#[must_use]
pub fn is_repo(repo: &str) -> bool {
    repo.len() <= 140 && plain_repo(repo)
}

/// Is this a full commit id: 40 hex digits, lower case (as GitHub and the engine write one). A short id is none: it
/// could come to name another commit.
#[must_use]
pub fn is_full_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The variables of [`PASS_THROUGH`] that `lookup` holds.
#[must_use]
pub fn extra_env(lookup: impl Fn(&str) -> Option<OsString>) -> Vec<(OsString, OsString)> {
    PASS_THROUGH
        .iter()
        .filter_map(|name| lookup(name).map(|value| (OsString::from(name), value)))
        .collect()
}

/// Where the release that was started last stands.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// It runs.
    Pending,
    /// The engine's document of the release (its `exit` says how it went).
    Done(Value),
    /// The step gave no document: a short code and a sentence for the page.
    Failed {
        code: &'static str,
        message: &'static str,
    },
}

/// The release that was started last: which commit, and where it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Last {
    pub repo: String,
    pub sha: String,
    pub outcome: Outcome,
}

/// Why a release was not started.
#[derive(Debug, PartialEq, Eq)]
pub enum BeginError {
    /// No engine to ask.
    Unavailable,
    /// Another step is under way.
    Busy,
    /// The review aid of this commit is not the one handed out last, or it is too old.
    NotShown,
}

/// Why a step gave no document.
#[derive(Debug)]
pub enum StepError {
    /// No engine to ask.
    Unavailable,
    /// Another step is under way.
    Busy,
    /// The child could not be started.
    Spawn(String),
    /// The step took longer than its limit; the child was ended.
    Timeout,
    /// What the child printed is no document of the contract.
    Protocol(&'static str),
}

impl StepError {
    /// A short code and a sentence for the page: nothing of what the child said.
    #[must_use]
    pub fn words(&self) -> (&'static str, &'static str) {
        match self {
            Self::Unavailable => (
                "release_unavailable",
                "this hub cannot release a commit: no engine",
            ),
            Self::Busy => ("busy", "another step of a release is under way"),
            Self::Spawn(_) => ("engine_spawn", "the engine could not be started"),
            Self::Timeout => (
                "engine_timeout",
                "the engine took too long (it asks GitHub through gh) and was ended; after a release, `testing ci trust --list` says whether the commit is on the list",
            ),
            Self::Protocol(_) => (
                "engine_protocol",
                "the engine gave no document of testing-ci-release/1 (testing.nvim too old?)",
            ),
        }
    }
}

/// The review aid that was handed out last.
#[derive(Debug)]
struct Shown {
    repo: String,
    sha: String,
    at: Instant,
}

/// The steps of the assistant: the program, the review aid that allows a release, the repositories a step may name.
pub struct Release {
    program: Option<StepProgram>,
    extra: Vec<(OsString, OsString)>,
    cwd: PathBuf,
    /// Folders of this machine a document of the engine may name in a `problem` (the list of reviewed commits).
    hidden: Vec<PathBuf>,
    shown_for: Duration,
    busy: Arc<tokio::sync::Mutex<()>>,
    shown: Mutex<Option<Shown>>,
    known: Mutex<Vec<String>>,
    last: Mutex<Option<Last>>,
}

impl Release {
    /// With the program of the engine (`None`: no step can be asked) and the variables of [`PASS_THROUGH`].
    #[must_use]
    pub fn new(program: Option<StepProgram>, extra: Vec<(OsString, OsString)>) -> Self {
        let var = |name: &str| {
            let of = |list: &[(OsString, OsString)]| {
                list.iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| PathBuf::from(v))
            };
            of(&extra).or_else(|| program.as_ref().and_then(|p| of(&p.env)))
        };
        let home = var("USERPROFILE").or_else(|| var("HOME"));
        let hidden = [var("TESTING_RUNNER_ROOT"), home]
            .into_iter()
            .flatten()
            .collect();
        Self {
            program,
            extra,
            // Not a checkout: the engine names the repository of "this folder" first, and the hub has none.
            cwd: std::env::temp_dir(),
            hidden,
            shown_for: SHOWN_FOR,
            busy: Arc::new(tokio::sync::Mutex::new(())),
            shown: Mutex::new(None),
            known: Mutex::new(Vec::new()),
            last: Mutex::new(None),
        }
    }

    /// With another life of a review aid (tests).
    #[must_use]
    pub fn with_shown_for(mut self, shown_for: Duration) -> Self {
        self.shown_for = shown_for;
        self
    }

    /// Can this hub ask a step?
    #[must_use]
    pub fn available(&self) -> bool {
        self.program.is_some()
    }

    /// The repositories the assistant offered last: only these a later step may name.
    pub fn set_known(&self, names: impl IntoIterator<Item = String>) {
        let mut known: Vec<String> = names.into_iter().map(|n| n.to_lowercase()).collect();
        known.sort();
        known.dedup();
        *self.known.lock().unwrap_or_else(PoisonError::into_inner) = known;
    }

    /// Was this repository offered (GitHub does not tell the case of a name apart)?
    #[must_use]
    pub fn is_known(&self, repo: &str) -> bool {
        let repo = repo.to_lowercase();
        self.known
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binary_search(&repo)
            .is_ok()
    }

    /// The review aid of this commit was handed out: it allows the release of exactly this commit, for a while.
    pub fn remember_shown(&self, repo: &str, sha: &str) {
        *self.shown.lock().unwrap_or_else(PoisonError::into_inner) = Some(Shown {
            repo: repo.to_lowercase(),
            sha: sha.to_owned(),
            at: Instant::now(),
        });
    }

    /// May this commit be released: its review aid is the one handed out last, and not long ago. A yes spends the
    /// aid; a no leaves what is there.
    #[must_use]
    pub fn take_shown(&self, repo: &str, sha: &str) -> bool {
        let mut shown = self.shown.lock().unwrap_or_else(PoisonError::into_inner);
        let fits = shown.as_ref().is_some_and(|s| {
            s.repo == repo.to_lowercase() && s.sha == sha && s.at.elapsed() <= self.shown_for
        });
        if fits {
            *shown = None;
        }
        fits
    }

    /// Ask the engine one step and read its document: an object of [`SCHEMA`] that names this step (or `usage`), with
    /// the folders of this machine taken out of its texts.
    ///
    /// # Errors
    ///
    /// [`StepError`]: no program, a step under way, no child, too slow, or no document.
    pub async fn run(&self, step: &Step) -> Result<Value, StepError> {
        if self.program.is_none() {
            return Err(StepError::Unavailable);
        }
        let Ok(_one_at_a_time) = self.busy.try_lock() else {
            return Err(StepError::Busy);
        };
        self.ask(step).await
    }

    /// The release that was started last, and where it stands; `None` before the first.
    #[must_use]
    pub fn last(&self) -> Option<Last> {
        self.last
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_last(&self, repo: &str, sha: &str, outcome: Outcome) {
        *self.last.lock().unwrap_or_else(PoisonError::into_inner) = Some(Last {
            repo: repo.to_owned(),
            sha: sha.to_owned(),
            outcome,
        });
    }

    /// Start the release of a commit: only when no step is under way and its review aid is the one handed out last
    /// (which this spends). It runs in the background to its end, whatever becomes of the request that started it;
    /// [`Release::last`] says where it stands, and `on_done` is called when it ended.
    ///
    /// # Errors
    ///
    /// [`BeginError`]: no program, a step under way, or no review aid of this commit.
    pub fn begin_release(
        self: &Arc<Self>,
        repo: &str,
        sha: &str,
        on_done: impl FnOnce() + Send + 'static,
    ) -> Result<(), BeginError> {
        if self.program.is_none() {
            return Err(BeginError::Unavailable);
        }
        let Ok(one_at_a_time) = Arc::clone(&self.busy).try_lock_owned() else {
            return Err(BeginError::Busy);
        };
        // After the lock: a release that cannot start now leaves the aid for the next try.
        if !self.take_shown(repo, sha) {
            return Err(BeginError::NotShown);
        }
        self.set_last(repo, sha, Outcome::Pending);
        let this = Arc::clone(self);
        let step = Step::Release {
            repo: repo.to_owned(),
            sha: sha.to_owned(),
        };
        tokio::spawn(async move {
            let outcome = match this.ask(&step).await {
                Ok(doc) => Outcome::Done(doc),
                Err(error) => {
                    match &error {
                        StepError::Spawn(detail) => {
                            eprintln!("release: the engine could not be started: {detail}");
                        }
                        StepError::Protocol(why) => {
                            eprintln!("release: the engine gave no document: {why}");
                        }
                        _ => {}
                    }
                    let (code, message) = error.words();
                    Outcome::Failed { code, message }
                }
            };
            if let Step::Release { repo, sha } = &step {
                this.set_last(repo, sha, outcome);
            }
            drop(one_at_a_time);
            on_done();
        });
        Ok(())
    }

    /// One step, asked of the engine; the caller holds the lock of "one step at a time".
    async fn ask(&self, step: &Step) -> Result<Value, StepError> {
        let Some(program) = &self.program else {
            return Err(StepError::Unavailable);
        };
        let mut command = tokio::process::Command::new(&program.program);
        command
            .args(&program.args)
            .args(step.args())
            .env_clear()
            .envs(program.env.iter().map(|(k, v)| (k, v)))
            .envs(self.extra.iter().map(|(k, v)| (k, v)))
            .current_dir(&self.cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW: no console window for a process the hub starts, as for its engine.
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| StepError::Spawn(e.to_string()))?;
        let Some(stdout) = child.stdout.take() else {
            return Err(StepError::Protocol("the child has no output"));
        };
        let read = async {
            let mut out = Vec::new();
            stdout
                .take(MAX_OUTPUT as u64 + 1)
                .read_to_end(&mut out)
                .await
                .map_err(|_| StepError::Protocol("the output could not be read"))?;
            // More than a document: the child is not waited for (it is ended when it is dropped).
            if out.len() > MAX_OUTPUT {
                return Ok(out);
            }
            child
                .wait()
                .await
                .map_err(|_| StepError::Protocol("the child did not end"))?;
            Ok::<_, StepError>(out)
        };
        // Too slow: the child is ended when it is dropped (`kill_on_drop`).
        let out = tokio::time::timeout(step.limit(), read)
            .await
            .map_err(|_| StepError::Timeout)??;
        self.document(step, &out)
    }

    /// The document in what a step printed.
    fn document(&self, step: &Step, out: &[u8]) -> Result<Value, StepError> {
        if out.len() > MAX_OUTPUT {
            return Err(StepError::Protocol("the document is too large"));
        }
        let text = std::str::from_utf8(out).map_err(|_| StepError::Protocol("not UTF-8"))?;
        let line = text
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .ok_or(StepError::Protocol("the engine printed nothing"))?;
        let mut doc: Value =
            serde_json::from_str(line).map_err(|_| StepError::Protocol("not JSON"))?;
        if !doc.is_object() || doc["schema"] != SCHEMA {
            return Err(StepError::Protocol(
                "not a document of testing-ci-release/1",
            ));
        }
        if doc["step"] != step.word() && doc["step"] != "usage" {
            return Err(StepError::Protocol("the document is of another step"));
        }
        if !doc["exit"].is_i64() {
            return Err(StepError::Protocol("the document names no exit code"));
        }
        for root in &self.hidden {
            hide_root(&mut doc, root);
        }
        Ok(doc)
    }

    /// The folders that are taken out of a document (tests).
    #[must_use]
    pub fn hidden(&self) -> &[PathBuf] {
        &self.hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn words(step: &Step) -> Vec<String> {
        step.args()
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_argv_of_a_step_is_fixed_and_always_asks_for_the_document() {
        assert_eq!(words(&Step::Repos), ["ci", "release", "--repos", "--json"]);
        assert_eq!(
            words(&Step::Commits { repo: "o/r".into() }),
            ["ci", "release", "o/r", "--commits", "--json"]
        );
        assert_eq!(
            words(&Step::Show {
                repo: "o/r".into(),
                sha: SHA.into()
            }),
            ["ci", "release", "o/r", SHA, "--show", "--json"]
        );
        assert_eq!(
            words(&Step::Release {
                repo: "o/r".into(),
                sha: SHA.into()
            }),
            ["ci", "release", "o/r", SHA, "--yes", "--json"]
        );
        // Only the release says yes, and no step names a folder, a note or a count.
        for step in [
            Step::Repos,
            Step::Commits { repo: "o/r".into() },
            Step::Show {
                repo: "o/r".into(),
                sha: SHA.into(),
            },
        ] {
            let argv = words(&step);
            assert!(!argv.contains(&"--yes".to_owned()), "{argv:?}");
            assert!(
                argv.iter()
                    .all(|a| !a.starts_with("--runners") && !a.starts_with("--note")),
                "{argv:?}"
            );
        }
    }

    #[test]
    fn a_commit_id_is_the_full_one_and_a_repository_is_owner_and_name() {
        assert!(is_full_sha(SHA));
        for bad in [
            "",
            "0123456",
            &SHA[..39],
            &format!("{SHA}0"),
            &SHA.to_uppercase(),
            "g123456789abcdef0123456789abcdef01234567",
            "main",
            "HEAD",
            "--yes",
            " 123456789abcdef0123456789abcdef01234567",
        ] {
            assert!(!is_full_sha(bad), "{bad:?}");
        }
        for ok in ["WKDSteve/testing.nvim", "o/r", "a-b/c_d.e"] {
            assert!(is_repo(ok), "{ok}");
        }
        for bad in [
            "",
            "o",
            "o/",
            "/r",
            "o/r/x",
            "-o/r",
            "o/-r",
            "o/--yes",
            "o/..",
            "o/r --yes",
            "o/r;x",
            "o\\r",
            "ö/r",
            &format!("o/{}", "r".repeat(140)),
        ] {
            assert!(!is_repo(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_release_needs_the_review_aid_of_exactly_that_commit_and_spends_it() {
        let release = Release::new(None, Vec::new());
        assert!(!release.take_shown("o/r", SHA), "nothing was shown");
        release.remember_shown("O/R", SHA);
        let other = "1123456789abcdef0123456789abcdef01234567";
        assert!(!release.take_shown("o/r", other), "another commit");
        assert!(!release.take_shown("o/other", SHA), "another repository");
        // A refusal left the aid in place; the name of a repository has no case.
        assert!(release.take_shown("o/r", SHA));
        assert!(!release.take_shown("o/r", SHA), "it is spent");
        // A later aid replaces the one before.
        release.remember_shown("o/r", SHA);
        release.remember_shown("o/r", other);
        assert!(!release.take_shown("o/r", SHA));
        assert!(release.take_shown("o/r", other));
    }

    #[test]
    fn a_review_aid_is_good_for_a_while_only() {
        let release = Release::new(None, Vec::new()).with_shown_for(Duration::ZERO);
        release.remember_shown("o/r", SHA);
        std::thread::sleep(Duration::from_millis(5));
        assert!(!release.take_shown("o/r", SHA));
    }

    #[test]
    fn a_step_names_only_a_repository_the_assistant_offered() {
        let release = Release::new(None, Vec::new());
        assert!(!release.is_known("o/r"));
        release.set_known(["WKDSteve/Testing.nvim".to_owned(), "o/r".to_owned()]);
        assert!(release.is_known("wkdsteve/testing.nvim"));
        assert!(release.is_known("O/R"));
        assert!(!release.is_known("o/rr"));
        release.set_known(Vec::new());
        assert!(!release.is_known("o/r"));
    }

    #[test]
    fn the_document_of_a_step_is_one_object_of_the_contract_of_that_step() {
        let release = Release::new(None, Vec::new());
        let step = Step::Repos;
        let good = br#"{"schema":"testing-ci-release/1","step":"repos","exit":0,"repos":[]}"#;
        assert_eq!(release.document(&step, good).unwrap()["step"], "repos");
        // What the engine says before its document does not count; the last line is the document.
        let noisy = [b"note: something\n".as_slice(), good, b"\n\n"].concat();
        assert!(release.document(&step, &noisy).is_ok());
        // A refusal of the arguments is a document too.
        let usage = br#"{"schema":"testing-ci-release/1","step":"usage","exit":2,"problem":"x"}"#;
        assert_eq!(release.document(&step, usage).unwrap()["exit"], 2);
        for bad in [
            b"".as_slice(),
            b"testing ci: unknown argument release",
            b"[]",
            br#"{"schema":"testing-ci-release/2","step":"repos","exit":0}"#,
            br#"{"schema":"testing-ci-release/1","step":"release","exit":0}"#,
            br#"{"schema":"testing-ci-release/1","step":"repos"}"#,
            b"\xff\xfe",
        ] {
            assert!(
                release.document(&step, bad).is_err(),
                "{:?}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn a_folder_of_this_machine_is_taken_out_of_a_document() {
        let home = if cfg!(windows) {
            r"C:\Users\someone"
        } else {
            "/home/someone"
        };
        let program = StepProgram {
            program: PathBuf::from("nvim"),
            args: Vec::new(),
            env: vec![(OsString::from("USERPROFILE"), OsString::from(home))],
            cwd: None,
        };
        let release = Release::new(Some(program), Vec::new());
        assert_eq!(release.hidden(), [PathBuf::from(home)]);
        let line = serde_json::json!({
            "schema": SCHEMA, "step": "repos", "exit": 3,
            "problem": format!("{home}/actions-runners/reviewed.json cannot be read"),
        })
        .to_string();
        let doc = release.document(&Step::Repos, line.as_bytes()).unwrap();
        assert_eq!(
            doc["problem"],
            "<path>/actions-runners/reviewed.json cannot be read"
        );
    }

    #[test]
    fn only_the_named_variables_come_along() {
        let env = extra_env(|name| match name {
            "TESTING_RUNNER_ROOT" => Some(OsString::from("/runners")),
            "GH_TOKEN" | "GITHUB_TOKEN" => Some(OsString::from("secret")),
            _ => None,
        });
        assert_eq!(
            env,
            [(
                OsString::from("TESTING_RUNNER_ROOT"),
                OsString::from("/runners")
            )]
        );
        assert!(!PASS_THROUGH.iter().any(|n| n.contains("TOKEN")));
    }
}
