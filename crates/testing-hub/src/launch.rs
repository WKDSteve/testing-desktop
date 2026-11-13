//! A run through the engine, started from the app (`run/*` routes, `launching`): the whole suite of a repository of
//! the fleet, the spec files of one name, or the test sheets bound to one task. The hub starts the engine's own command
//! line as a child, never a run of its own:
//!
//! * the suite: `testing run --root <root>` (a heavy run: it takes its turn in the engine's queue of heavy runs, the
//!   machine-wide run lock, across every session of the person);
//! * a file: `testing run --root <root> --file <text>` (a selected run, which the engine never queues);
//! * the sheets of a task: `testing sheets run --task <area/slug> --root <root>` (only sheets that name the task run).
//!
//! The result is where every run of the engine leaves it, in its state folder: the hub's watcher sees it there and
//! `repo` reads it (`testing call repo`), one way to the result for a terminal, the app and the hub alike. The hub only
//! keeps what it needs to say whether a run is under way and how it ended.
//!
//! What holds here, each with a test:
//!
//! * one run per hub: a second start while one runs is refused and names what runs;
//! * the argv is fixed: a word of a closed list, the root the engine named for a key of the fleet (never a path of a
//!   request), a file name of plain characters, a task id of plain characters; none can begin with `-`;
//! * the engine's own rules decide the rest: the trust of a root (the hub never passes `--trust`), the workstation,
//!   no sheet that names the task (exit 2, `refused`);
//! * the hub knows the sheets bound to a task ([`task_sheets`]: the `task = "<id>"` line of a sheet, read as text as
//!   the engine reads a sheet that does not load) and starts a task's run only when there is one;
//! * what the child writes is kept as a short tail, with the root taken out, for the PC only;
//! * the run ends with the hub, and after [`RUN_LIMIT`].

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::oneshot;

use crate::sheetstep::{is_sheet_name, scrub, StepProgram};
use crate::store::SnapshotStore;

/// The longest file text of a run of one name (`--file`).
pub const MAX_FILE_TEXT: usize = 100;

/// The longest task id (`area/slug`): what a query value of the kit may hold.
pub const MAX_TASK_ID: usize = 128;

/// The largest sheet file the hub reads for its `task` line; a sheet is a few KiB.
pub const MAX_SHEET_BYTES: u64 = 256 * 1024;

/// How long a run may take, its wait in the engine's queue of heavy runs included (the engine gives up waiting after
/// four hours).
pub const RUN_LIMIT: Duration = Duration::from_secs(5 * 60 * 60);

/// How much of the child's output is kept: the end of it, where the verdict stands.
const TAIL_BYTES: usize = 16 * 1024;

/// What runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum What {
    /// The whole suite of the repository.
    Suite,
    /// The spec files whose name holds this text.
    File(String),
    /// The test sheets that name this task.
    Task(String),
}

impl What {
    /// The word of the kind of run, as the state names it.
    #[must_use]
    pub fn word(&self) -> &'static str {
        match self {
            Self::Suite => "suite",
            Self::File(_) => "file",
            Self::Task(_) => "task",
        }
    }

    /// The arguments after the engine's script: a fixed command of the engine, the root, and the one value of the
    /// request (checked by [`is_file_text`] or [`is_task_id`]). A run of specs records its result in `state_dir`, the
    /// state folder the hub reads (a setting of the person, never of a request); without one, in the state folder of
    /// the engine's editor. A run of sheets keeps its results where `testing sheets report` reads them.
    #[must_use]
    pub fn args(&self, root: &Path, state_dir: Option<&Path>) -> Vec<OsString> {
        let mut args: Vec<OsString> = Vec::new();
        match self {
            Self::Suite => args.extend(["run".into(), "--root".into(), root.into()]),
            Self::File(text) => args.extend([
                "run".into(),
                "--root".into(),
                root.into(),
                "--file".into(),
                text.into(),
            ]),
            Self::Task(task) => args.extend([
                "sheets".into(),
                "run".into(),
                "--task".into(),
                task.into(),
                "--root".into(),
                root.into(),
            ]),
        }
        if let (Self::Suite | Self::File(_), Some(dir)) = (self, state_dir) {
            args.extend(["--state-dir".into(), dir.into()]);
        }
        args
    }
}

/// Is this a file text the engine may be given for `--file`: part of a file name (the engine matches it literally),
/// of plain characters, nothing that could be an option or climb.
#[must_use]
pub fn is_file_text(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_FILE_TEXT
        && !text.starts_with(['-', '.', '/'])
        && !text.contains("..")
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'/'))
}

/// Is this the id of a task (`<area>/<slug>`, as a sheet names it): two parts of letters, digits, `_`, `-` and `.`,
/// split by one `/`, neither beginning with `.` or `-`.
#[must_use]
pub fn is_task_id(id: &str) -> bool {
    let part = |p: &str| {
        !p.is_empty()
            && !p.starts_with(['.', '-'])
            && !p.contains("..")
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
    };
    id.len() <= MAX_TASK_ID
        && id
            .split_once('/')
            .is_some_and(|(area, slug)| part(area) && part(slug))
}

/// The area of a task id: the part before the `/`.
#[must_use]
pub fn task_area(id: &str) -> &str {
    id.split_once('/').map_or(id, |(area, _)| area)
}

/// Does the text of a sheet name this task: a line of its own `task = "<id>"` (single or double quotes, blanks, a
/// trailing comma), the rule of the engine for the task of a sheet (testing.nvim `testing.sheet.tasks`).
#[must_use]
pub fn names_task(text: &str, id: &str) -> bool {
    text.lines().any(|line| {
        let Some(rest) = line.trim_start().strip_prefix("task") else {
            return false;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            return false;
        };
        let rest = rest.trim_start();
        let Some(quote) = rest.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
            return false;
        };
        let Some((value, after)) = rest[1..].split_once(quote) else {
            return false;
        };
        let after = after.trim();
        value == id && (after.is_empty() || after == ",")
    })
}

/// The sheets of a repository that name this task: the names (without `.sheet.lua`) of the files below
/// `TESTS/sheets` whose text has its `task` line, sorted. A repository without the folder has none; a file too large
/// for a sheet is not read.
///
/// # Errors
///
/// The folder is there but cannot be read.
pub fn task_sheets(root: &Path, id: &str) -> std::io::Result<Vec<String>> {
    let dir = root.join("TESTS").join("sheets");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&dir)?.filter_map(Result::ok) {
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(base) = name.strip_suffix(".sheet.lua").filter(|b| is_sheet_name(b)) else {
            continue;
        };
        if entry.metadata().is_ok_and(|m| m.len() > MAX_SHEET_BYTES) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        if names_task(&text, id) {
            names.push(base.to_owned());
        }
    }
    names.sort();
    Ok(names)
}

/// The verdict of an exit code of the engine (`docs/EXIT-CODES.md` of testing.nvim): `0` green, `1` red, `2` refused
/// (usage, trust, the workstation, no sheet of the task), `3` the run could not be made; no code: the child was
/// stopped.
#[must_use]
pub fn verdict(code: Option<i32>) -> &'static str {
    match code {
        Some(0) => "green",
        Some(1) => "red",
        Some(2) => "refused",
        Some(_) => "failed",
        None => "stopped",
    }
}

/// Why a run was not started.
#[derive(Debug)]
pub enum StartError {
    /// No engine to run.
    Unavailable,
    /// A run is under way: what it is.
    Busy { what: &'static str, key: String },
    /// The child could not be started.
    Spawn(String),
}

/// The one run of the hub.
#[derive(Debug)]
struct Run {
    id: u64,
    key: String,
    what: What,
    root: PathBuf,
    running: bool,
    exit: Option<i32>,
    started: u64,
    ended: Option<u64>,
    output: Vec<u8>,
    kill: Option<oneshot::Sender<()>>,
}

/// Runs through the engine: the program, and the run that is or was last.
pub struct Launch {
    program: Option<StepProgram>,
    store: Option<Arc<SnapshotStore>>,
    state_dir: Option<PathBuf>,
    run: Mutex<Option<Run>>,
    counter: AtomicU64,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Launch {
    /// With the program of the engine (`None`: nothing can be run), the snapshot that is read again when a run ends,
    /// and the state folder the hub reads, where a run of specs records its result.
    #[must_use]
    pub fn new(
        program: Option<StepProgram>,
        store: Option<Arc<SnapshotStore>>,
        state_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            program,
            store,
            state_dir,
            run: Mutex::new(None),
            counter: AtomicU64::new(0),
        }
    }

    /// Can this hub start a run?
    #[must_use]
    pub fn available(&self) -> bool {
        self.program.is_some()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Run>> {
        self.run.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Where the run stands, for the PC: `run` is `null` before the first one.
    #[must_use]
    pub fn state(&self) -> Value {
        let guard = self.lock();
        let run = guard.as_ref().map(|r| {
            let mut v = json!({
                "id": r.id,
                "key": r.key,
                "what": r.what.word(),
                "running": r.running,
                "exit": r.exit,
                "verdict": (!r.running).then(|| verdict(r.exit)),
                "started": r.started,
                "ended": r.ended,
                "output": scrub_tail(&r.output, &r.root),
            });
            match &r.what {
                What::File(text) => v["file"] = json!(text),
                What::Task(task) => v["task"] = json!(task),
                What::Suite => {}
            }
            v
        });
        json!({ "schema": 1, "kind": "hub.run", "available": self.available(), "run": run })
    }

    /// Where the run stands, for a paired device: a run of a task's sheets as it is (without the output, which may
    /// name folders of this PC); of any other run only that one is under way, nothing of its repository.
    #[must_use]
    pub fn state_for_device(&self) -> Value {
        let mut state = self.state();
        if let Some(run) = state.get_mut("run").filter(|r| r.is_object()) {
            if run["what"] == "task" {
                if let Some(map) = run.as_object_mut() {
                    map.remove("output");
                }
            } else {
                *run = json!({ "what": "other", "running": run["running"] });
            }
        }
        state
    }

    /// Start a run of `what` in `root`, the repository with the key `key`. Answers at once with the state; the run
    /// goes on in the background.
    ///
    /// # Errors
    ///
    /// [`StartError`]: no program, a run under way, no child.
    pub fn start(
        self: &Arc<Self>,
        key: &str,
        what: What,
        root: &Path,
    ) -> Result<Value, StartError> {
        let Some(program) = &self.program else {
            return Err(StartError::Unavailable);
        };
        let mut guard = self.lock();
        if let Some(r) = guard.as_ref().filter(|r| r.running) {
            return Err(StartError::Busy {
                what: r.what.word(),
                key: r.key.clone(),
            });
        }
        let mut command = tokio::process::Command::new(&program.program);
        command
            .args(&program.args)
            .args(what.args(root, self.state_dir.as_deref()))
            .env_clear()
            .envs(program.env.iter().map(|(k, v)| (k, v)))
            // A run of the command line starts in the root, as `scripts/test.sh` does.
            .current_dir(root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW: no console window for a process the hub starts, as for its engine.
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| StartError::Spawn(scrub(&e.to_string(), &[root])))?;
        let id = self.counter.fetch_add(1, Ordering::AcqRel) + 1;
        let (kill_tx, kill_rx) = oneshot::channel();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        *guard = Some(Run {
            id,
            key: key.to_owned(),
            what,
            root: root.to_path_buf(),
            running: true,
            exit: None,
            started: now(),
            ended: None,
            output: Vec::new(),
            kill: Some(kill_tx),
        });
        drop(guard);
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let read_out = this.read_into(id, stdout);
            let read_err = this.read_into(id, stderr);
            let wait = async {
                tokio::select! {
                    status = child.wait() => status.ok().and_then(|s| s.code()),
                    _ = kill_rx => {
                        let _ = child.kill().await;
                        None
                    }
                    () = tokio::time::sleep(RUN_LIMIT) => {
                        let _ = child.kill().await;
                        None
                    }
                }
            };
            let (code, (), ()) = tokio::join!(wait, read_out, read_err);
            {
                let mut guard = this.lock();
                if let Some(r) = guard.as_mut().filter(|r| r.id == id) {
                    r.running = false;
                    r.exit = code;
                    r.ended = Some(now());
                    r.kill = None;
                }
            }
            // The result is in the state folder now: the fleet is read again.
            if let Some(store) = &this.store {
                store.request_rescan();
            }
        });
        Ok(self.state())
    }

    /// Read a stream of the child into the tail of run `id` until it ends.
    async fn read_into<R: AsyncRead + Unpin>(&self, id: u64, stream: Option<R>) {
        let Some(mut stream) = stream else {
            return;
        };
        let mut buf = vec![0u8; 8192];
        loop {
            let n = match stream.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            let mut guard = self.lock();
            let Some(r) = guard.as_mut().filter(|r| r.id == id) else {
                return;
            };
            r.output.extend_from_slice(&buf[..n]);
            if r.output.len() > TAIL_BYTES {
                let cut = r.output.len() - TAIL_BYTES;
                r.output.drain(..cut);
            }
        }
    }

    /// The hub stops: a run under way is killed (the engine's child editors end with it, as when a terminal closes).
    pub fn stop(&self) {
        let kill = self.lock().as_mut().and_then(|r| r.kill.take());
        if let Some(kill) = kill {
            let _ = kill.send(());
        }
    }
}

/// The tail of the output as text for the page: the root taken out, nothing unprintable, the last lines.
fn scrub_tail(output: &[u8], root: &Path) -> String {
    let text = String::from_utf8_lossy(output);
    // `scrub` keeps the first 600 characters; the verdict is at the end, so the end is given to it.
    let chars: Vec<char> = text.chars().collect();
    let start = chars.len().saturating_sub(2000);
    let tail: String = chars[start..].iter().collect();
    let scrubbed = scrub(&tail, &[root]);
    let lines: Vec<&str> = scrubbed.lines().collect();
    let from = lines.len().saturating_sub(20);
    lines[from..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_argv_is_a_fixed_command_the_root_and_one_checked_value() {
        let root = Path::new("/repos/alpha.nvim");
        let words = |what: What| -> Vec<String> {
            what.args(root, None)
                .into_iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(words(What::Suite), ["run", "--root", "/repos/alpha.nvim"]);
        assert_eq!(
            words(What::File("config".into())),
            ["run", "--root", "/repos/alpha.nvim", "--file", "config"]
        );
        assert_eq!(
            words(What::Task("alpha.nvim/try-it".into())),
            [
                "sheets",
                "run",
                "--task",
                "alpha.nvim/try-it",
                "--root",
                "/repos/alpha.nvim"
            ]
        );
        // The result of specs goes where the hub reads; sheets keep theirs where `sheets report` reads them.
        let state = Path::new("/state/nvim");
        let with_state = |what: What| -> Vec<String> {
            what.args(root, Some(state))
                .into_iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            with_state(What::File("config".into()))[5..],
            ["--state-dir", "/state/nvim"]
        );
        assert_eq!(with_state(What::Suite)[3..], ["--state-dir", "/state/nvim"]);
        assert!(!with_state(What::Task("a/b".into())).contains(&"--state-dir".to_owned()));
        // Never the trust of a root, never a CI command.
        for what in [
            What::Suite,
            What::File("x".into()),
            What::Task("a/b".into()),
        ] {
            for word in words(what) {
                assert!(!word.contains("trust") && word != "ci", "{word}");
            }
        }
    }

    #[test]
    fn a_file_text_is_plain_and_no_option() {
        for ok in ["config", "ui_spec", "sub/dir_spec.lua", "a-b.c"] {
            assert!(is_file_text(ok), "{ok}");
        }
        for bad in [
            "",
            "-x",
            "--trust",
            ".hidden",
            "/abs",
            "a/../b",
            "a b",
            "a;b",
            "a\\b",
            "ä",
            &"a".repeat(MAX_FILE_TEXT + 1),
        ] {
            assert!(!is_file_text(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_task_id_is_an_area_and_a_slug() {
        for ok in [
            "testing-desktop/lauf-starten",
            "ui.nvim/try-the-ui-option-float-in-a-real-session",
            "ALL/x_1",
        ] {
            assert!(is_task_id(ok), "{ok}");
        }
        for bad in [
            "",
            "area",
            "/slug",
            "area/",
            "a/b/c",
            "-a/b",
            "a/-b",
            "../b",
            "a/..",
            ".a/b",
            "a b/c",
            "a/b;c",
            &format!("a/{}", "b".repeat(MAX_TASK_ID)),
        ] {
            assert!(!is_task_id(bad), "{bad:?}");
        }
        assert_eq!(task_area("ui.nvim/try"), "ui.nvim");
    }

    #[test]
    fn a_sheet_names_its_task_in_its_task_line_and_nowhere_else() {
        let id = "ui.nvim/try-it";
        for yes in [
            "return {\n  task = \"ui.nvim/try-it\",\n}",
            "  task='ui.nvim/try-it'",
            "task   =   \"ui.nvim/try-it\" , ",
        ] {
            assert!(names_task(yes, id), "{yes}");
        }
        for no in [
            "task = \"ui.nvim/try-it-more\"",
            "-- task = \"ui.nvim/other\"",
            "title = \"ui.nvim/try-it\"",
            "tasks = \"ui.nvim/try-it\"",
            "task = ui.nvim/try-it",
            "task = \"ui.nvim/try-it",
            "task = \"ui.nvim/try-it\" -- a comment",
            "return { task = \"ui.nvim/try-it\" }",
            "",
        ] {
            assert!(!names_task(no, id), "{no}");
        }
    }

    #[test]
    fn the_sheets_of_a_task_are_those_whose_file_names_it() {
        let dir = appkit_scratch::Scratch::new("launch-sheets");
        let sheets = dir.join("TESTS").join("sheets");
        std::fs::create_dir_all(&sheets).unwrap();
        let write = |name: &str, text: &str| std::fs::write(sheets.join(name), text).unwrap();
        write(
            "b.sheet.lua",
            "return {\n  title = \"B\",\n  task = \"x/y\",\n}\n",
        );
        write("a.sheet.lua", "return {\n  task = 'x/y',\n}\n");
        write("other.sheet.lua", "return {\n  task = \"x/z\",\n}\n");
        // On one line with the rest of the table: the engine does not read it as the task line either.
        write("inline.sheet.lua", "return { task = \"x/y\" }\n");
        write("none.sheet.lua", "return {}\n");
        write("notes.md", "task = \"x/y\"\n");
        write("-bad.sheet.lua", "task = \"x/y\"\n");
        let big = format!("task = \"x/y\"\n{}", "-".repeat(MAX_SHEET_BYTES as usize));
        write("big.sheet.lua", &big);
        assert_eq!(task_sheets(&dir, "x/y").unwrap(), ["a", "b"]);
        assert_eq!(task_sheets(&dir, "x/z").unwrap(), ["other"]);
        assert!(task_sheets(&dir, "x/none").unwrap().is_empty());
        assert!(task_sheets(&dir.join("missing"), "x/y").unwrap().is_empty());
    }

    #[test]
    fn the_exit_code_of_the_engine_is_the_verdict() {
        assert_eq!(verdict(Some(0)), "green");
        assert_eq!(verdict(Some(1)), "red");
        assert_eq!(verdict(Some(2)), "refused");
        assert_eq!(verdict(Some(3)), "failed");
        assert_eq!(verdict(None), "stopped");
    }

    #[test]
    fn a_device_sees_a_task_run_without_its_output_and_of_any_other_only_that_it_runs() {
        let launch = Launch::new(None, None, None);
        assert_eq!(launch.state_for_device()["run"], Value::Null);
        let set = |what: What| {
            *launch.lock() = Some(Run {
                id: 1,
                key: "alpha.nvim-0123456789ab".into(),
                what,
                root: PathBuf::from("/repos/alpha.nvim"),
                running: false,
                exit: Some(1),
                started: 1,
                ended: Some(2),
                output: b"/repos/alpha.nvim/TESTS/x_spec.lua:3: red".to_vec(),
                kill: None,
            });
        };
        set(What::Suite);
        let pc = launch.state();
        assert_eq!(pc["run"]["verdict"], "red");
        assert_eq!(pc["run"]["output"], "<path>/TESTS/x_spec.lua:3: red");
        assert_eq!(
            launch.state_for_device()["run"],
            json!({ "what": "other", "running": false })
        );
        set(What::Task("alpha.nvim/try".into()));
        let device = launch.state_for_device();
        assert_eq!(device["run"]["task"], "alpha.nvim/try");
        assert_eq!(device["run"]["verdict"], "red");
        assert!(device["run"].get("output").is_none(), "{device}");
    }
}
