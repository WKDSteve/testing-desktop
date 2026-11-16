//! A run through the engine, started from the app (`run/*` routes, `launching`): the whole suite of a repository of
//! the fleet, the spec files of one name, or the test sheets bound to one task. The hub starts the engine's own command
//! line as a child, never a run of its own:
//!
//! * the suite: `testing run --root <root>` (a heavy run: it takes its turn in the engine's queue of heavy runs, the
//!   machine-wide run lock, across every session of the person);
//! * a file: `testing run --root <root> --file <text>` (a selected run, which the engine never queues);
//! * the sheets of a task: `testing sheets run --task <area/slug> --root <root> --result` (only sheets that name the
//!   task run; `--result` makes the engine say in a document of `testing-sheet-run/1` what ran and what was written),
//!   with `--tasks <vault>` when the person named a vault for this hub (`tasks_vault`): the engine then writes the
//!   result into the task's block, as `:Tasks sheet-run` does.
//!
//! The result is where every run of the engine leaves it, in its state folder: the hub's watcher sees it there and
//! `repo` reads it (`testing call repo`), one way to the result for a terminal, the app and the hub alike. The hub only
//! keeps what it needs to say whether a run is under way and how it ended.
//!
//! What holds here, each with a test:
//!
//! * one run per hub: a second start while one runs is refused and names what runs;
//! * the argv is fixed: a word of a closed list, the root the engine named for a key of the fleet (never a path of a
//!   request), a file name of plain characters, a task id of plain characters; none can begin with `-`; the vault
//!   of `--tasks` is a setting of the person, never of a request;
//! * the engine's own rules decide the rest: the trust of a root (the hub never passes `--trust`), the workstation,
//!   no sheet that names the task (exit 2, `refused`);
//! * the hub knows the sheets bound to a task ([`task_sheets`]: the `task = "<id>"` line of a sheet, read as text as
//!   the engine reads a sheet that does not load) and starts a task's run only when there is one;
//! * what the child writes is kept as a short tail, with the root taken out, for the PC only;
//! * the run ends with the hub, after [`RUN_LIMIT`], and when the page aborts it (`run/abort`); then everything it
//!   started ends with it (the engine is a child of the hub through `appkit_core::child`: a job object on Windows,
//!   its own process group elsewhere, ended with `killpg`; its child editors are not left running). `testing run` has
//!   no gentle end to ask for (its input is closed, it reads no command), so the tree is ended at once; the engine
//!   takes over the lock of a heavy run whose process is gone;
//! * a run that ends by itself leaves nothing behind either: what it left running is ended with it, and does not
//!   hold the run.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use appkit_core::child::{self, Environment, Start};
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

/// The largest document of a run of sheets the hub reads: one line of the engine's stdout. A larger one is read as
/// none and dropped whole, never shown.
pub const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;

/// The engine's stdout in a run of a task's sheets, line by line, kept apart from stderr: a line that begins with `{`
/// may be the engine's document ([`SHEET_RUN_SCHEMA`]) and is held until it is whole, then read; it never goes into
/// the output for the page. Every other line goes on as it comes.
#[derive(Debug, Default)]
struct Document {
    /// The line held so far.
    line: Vec<u8>,
    /// The line under way is held (it began with `{`).
    holding: bool,
    /// The line under way grew past [`MAX_DOCUMENT_BYTES`]: it is dropped.
    overflow: bool,
    /// Something of the line under way has come: the first byte decided whether it is held.
    begun: bool,
    /// The last document of the run read whole.
    doc: Option<Value>,
}

impl Document {
    /// Take a piece of stdout: whole lines that are not held go to `out`.
    fn feed(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
        for piece in bytes.split_inclusive(|b| *b == b'\n') {
            if !self.begun {
                self.begun = true;
                self.holding = piece[0] == b'{';
            }
            let whole = piece.ends_with(b"\n");
            if self.holding {
                if !self.overflow {
                    self.line.extend_from_slice(piece);
                    if self.line.len() > MAX_DOCUMENT_BYTES {
                        self.overflow = true;
                        self.line = Vec::new();
                    }
                }
                if whole {
                    self.finish(out, true);
                }
            } else {
                out.extend_from_slice(piece);
                if whole {
                    self.begun = false;
                }
            }
        }
    }

    /// The line under way is over: `whole` at its newline, else at the end of stdout. A held line is read as the
    /// document, or goes on as text when it is none (other JSON the engine printed); a held line that stdout cut off
    /// (the run ended in the middle of it) may be the start of a document and is dropped, and so is one that grew too
    /// long.
    fn finish(&mut self, out: &mut Vec<u8>, whole: bool) {
        if self.holding && !self.overflow {
            match sheet_result(&String::from_utf8_lossy(&self.line)) {
                Some(doc) => self.doc = Some(doc),
                None if whole => out.extend_from_slice(&self.line),
                None => {}
            }
        }
        *self = Self {
            doc: self.doc.take(),
            ..Self::default()
        };
    }
}

/// How long the last lines of a run that ended are read. What it left behind is ended with it, so the pipes close at
/// once; this is the bound should one not (a process that left the job or the group on purpose).
const DRAIN: Duration = Duration::from_secs(2);

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
    /// the engine's editor. A run of sheets keeps its results where `testing sheets report` reads them; with
    /// `tasks_vault` (a setting of the person too) the engine also writes them into the task's block.
    #[must_use]
    pub fn args(
        &self,
        root: &Path,
        state_dir: Option<&Path>,
        tasks_vault: Option<&Path>,
    ) -> Vec<OsString> {
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
                // The last line of the engine's stdout is then its own document of the run ([`sheet_result`]).
                "--result".into(),
            ]),
        }
        if let (Self::Suite | Self::File(_), Some(dir)) = (self, state_dir) {
            args.extend(["--state-dir".into(), dir.into()]);
        }
        if let (Self::Task(_), Some(vault)) = (self, tasks_vault) {
            args.extend(["--tasks".into(), vault.into()]);
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

/// The schema of the document `testing sheets run --result` prints as its last line.
pub const SHEET_RUN_SCHEMA: &str = "testing-sheet-run/1";

/// What the engine said of a run of sheets: the document of [`SHEET_RUN_SCHEMA`] in the last line of its kind of the
/// output. `None`: it printed none (the run was refused or did not start, an engine that does not know `--result`),
/// or a higher schema this hub does not read.
#[must_use]
pub fn sheet_result(output: &str) -> Option<Value> {
    output.lines().rev().find_map(|line| {
        let line = line.trim();
        if !line.starts_with('{') {
            return None;
        }
        let doc: Value = serde_json::from_str(line).ok()?;
        (doc["schema"] == SHEET_RUN_SCHEMA).then_some(doc)
    })
}

/// The verdict of a run: that of its exit code, read with the engine's document where there is one. The engine ends
/// with exit 1 for a red sheet and for a task it could not write alike; when it says no sheet is red and the task was
/// not written, the verdict is `not_written`, not `red`.
#[must_use]
pub fn verdict_of(code: Option<i32>, result: Option<&Value>) -> &'static str {
    match (code, result) {
        (Some(1), Some(doc)) if doc["red"] == false && doc["written"] == false => "not_written",
        _ => verdict(code),
    }
}

/// What became of the block of `task`, from the engine's document: whether it stands as the run left it and the
/// engine's word for it (a plain word; `unreachable` when tasks.nvim did not answer). `None`: nothing was to be
/// written (no vault), or there is no document.
#[must_use]
pub fn block_of(result: Option<&Value>, task: &str) -> Option<Value> {
    let doc = result?;
    let written = doc["written"].as_bool()?;
    let said = doc["tasks"]
        .as_array()
        .and_then(|tasks| tasks.iter().find(|t| t["id"] == task))
        .and_then(|t| t["outcome"].as_str())
        .filter(|word| {
            (1..=32).contains(&word.len())
                && word.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        });
    let outcome = said.unwrap_or(if written { "unchanged" } else { "unreachable" });
    Some(json!({ "written": written, "outcome": outcome }))
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
    /// The engine's stdout of a run of a task's sheets, with its document.
    document: Document,
    kill: Option<oneshot::Sender<()>>,
}

/// Runs through the engine: the program, and the run that is or was last.
pub struct Launch {
    program: Option<StepProgram>,
    store: Option<Arc<SnapshotStore>>,
    state_dir: Option<PathBuf>,
    tasks_vault: Option<PathBuf>,
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
    /// the state folder the hub reads, where a run of specs records its result, and the vault of the tasks, into
    /// which a run of a task's sheets writes its result (`None`: it writes into no task).
    #[must_use]
    pub fn new(
        program: Option<StepProgram>,
        store: Option<Arc<SnapshotStore>>,
        state_dir: Option<PathBuf>,
        tasks_vault: Option<PathBuf>,
    ) -> Self {
        Self {
            program,
            store,
            state_dir,
            tasks_vault,
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
            // What the engine said of a task's sheets, once they ended.
            let result = match &r.what {
                What::Task(_) if !r.running => r.document.doc.clone(),
                _ => None,
            };
            let mut v = json!({
                "id": r.id,
                "key": r.key,
                "what": r.what.word(),
                "running": r.running,
                "exit": r.exit,
                "verdict": (!r.running).then(|| verdict_of(r.exit, result.as_ref())),
                "started": r.started,
                "ended": r.ended,
                "output": scrub_tail(&r.output, &r.root, self.tasks_vault.as_deref()),
            });
            match &r.what {
                What::File(text) => v["file"] = json!(text),
                What::Task(task) => {
                    v["task"] = json!(task);
                    // What became of the task's block and whether a sheet is red, as the engine said it. A
                    // device hears both: words of the engine, no text and no path.
                    v["block"] = block_of(result.as_ref(), task).unwrap_or(Value::Null);
                    v["sheets_red"] = result
                        .as_ref()
                        .map_or(Value::Null, |doc| doc["red"].clone());
                }
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
        // A child of the hub (appkit_core::child): no console window, bound to the hub (a job object on Windows, a
        // process group elsewhere, ended with killpg), named in its log. Its stdin is closed at once.
        let start = Start::as_given("testing-hub run", &program.program)
            .args(&program.args)
            .args(what.args(root, self.state_dir.as_deref(), self.tasks_vault.as_deref()))
            .env(Environment::Only(program.env.clone()))
            // A run of the command line starts in the root, as `scripts/test.sh` does.
            .cwd(root);
        let mut child =
            child::spawn(&start).map_err(|e| StartError::Spawn(scrub(&e.to_string(), &[root])))?;
        drop(child.take_stdin());
        let id = self.counter.fetch_add(1, Ordering::AcqRel) + 1;
        let (kill_tx, kill_rx) = oneshot::channel();
        let stdout = child.take_stdout();
        let stderr = child.take_stderr();
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
            document: Document::default(),
            kill: Some(kill_tx),
        });
        drop(guard);
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut read_out = tokio::spawn(Arc::clone(&this).read_into(id, stdout, true));
            let mut read_err = tokio::spawn(Arc::clone(&this).read_into(id, stderr, false));
            let code = async {
                tokio::select! {
                    // Ended by itself: what it left running is ended too (`Leftovers::End` of the kit).
                    status = child.wait() => status.ok().and_then(|s| s.code()),
                    _ = kill_rx => {
                        child.end().await;
                        None
                    }
                    () = tokio::time::sleep(RUN_LIMIT) => {
                        child.end().await;
                        None
                    }
                }
            }
            .await;
            // The last lines, for a short while: should a process of the run have escaped its end, it holds the pipes
            // for as long as it lives, and it does not hold the run.
            let _ = tokio::time::timeout(DRAIN, async {
                let _ = (&mut read_out).await;
                let _ = (&mut read_err).await;
            })
            .await;
            read_out.abort();
            read_err.abort();
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

    /// Read a stream of the child into the tail of run `id` until it ends. The stdout of a run of a task's sheets goes
    /// through its [`Document`] first: the engine's document comes whole on stdout, whatever stderr does meanwhile.
    async fn read_into<R: AsyncRead + Unpin>(
        self: Arc<Self>,
        id: u64,
        stream: Option<R>,
        stdout: bool,
    ) {
        let Some(mut stream) = stream else {
            return;
        };
        let mut buf = vec![0u8; 8192];
        loop {
            let read = stream.read(&mut buf).await;
            let mut guard = self.lock();
            let Some(r) = guard.as_mut().filter(|r| r.id == id) else {
                return;
            };
            let sheets = stdout && matches!(r.what, What::Task(_));
            let n = match read {
                Ok(0) | Err(_) => {
                    if sheets {
                        r.document.finish(&mut r.output, false);
                    }
                    return;
                }
                Ok(n) => n,
            };
            if sheets {
                r.document.feed(&buf[..n], &mut r.output);
            } else {
                r.output.extend_from_slice(&buf[..n]);
            }
            if r.output.len() > TAIL_BYTES {
                let cut = r.output.len() - TAIL_BYTES;
                r.output.drain(..cut);
            }
        }
    }

    /// Abort the run under way: the engine and everything it started are ended, and the run ends as `stopped`.
    /// `false`: no run is under way. A second abort of the same run finds it still under way and changes nothing.
    pub fn abort(&self) -> bool {
        let mut guard = self.lock();
        let Some(run) = guard.as_mut().filter(|r| r.running) else {
            return false;
        };
        if let Some(kill) = run.kill.take() {
            let _ = kill.send(());
        }
        true
    }

    /// The hub stops: a run under way is ended with everything it started.
    pub fn stop(&self) {
        self.abort();
    }
}

/// The tail of the output as text for the page: the root and the vault of the tasks taken out, nothing unprintable,
/// the last lines. The engine's document of a run of sheets is not among them: it is for the hub, not for a person,
/// and as JSON it would name the folders of this PC in a spelling the scrub does not know.
fn scrub_tail(output: &[u8], root: &Path, vault: Option<&Path>) -> String {
    let text = String::from_utf8_lossy(output);
    let text: String = text
        .lines()
        .filter(|line| !line.contains(SHEET_RUN_SCHEMA))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    // `scrub` keeps the first 600 characters; the verdict is at the end, so the end is given to it.
    let chars: Vec<char> = text.chars().collect();
    let start = chars.len().saturating_sub(2000);
    let tail: String = chars[start..].iter().collect();
    let roots: Vec<&Path> = std::iter::once(root).chain(vault).collect();
    let scrubbed = scrub(&tail, &roots);
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
            what.args(root, None, None)
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
                "/repos/alpha.nvim",
                "--result"
            ]
        );
        // The result of specs goes where the hub reads; sheets keep theirs where `sheets report` reads them.
        let state = Path::new("/state/nvim");
        let with_state = |what: What| -> Vec<String> {
            what.args(root, Some(state), None)
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
        // The vault of the tasks goes to the run of a task's sheets and to no other; without one, no `--tasks`.
        let vault = Path::new("/vaults/tasks");
        let with_vault = |what: What| -> Vec<String> {
            what.args(root, Some(state), Some(vault))
                .into_iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            with_vault(What::Task("a/b".into()))[7..],
            ["--tasks", "/vaults/tasks"]
        );
        for what in [What::Suite, What::File("x".into())] {
            assert!(!with_vault(what).contains(&"--tasks".to_owned()));
        }
        assert!(!words(What::Task("a/b".into())).contains(&"--tasks".to_owned()));
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
    fn an_abort_finds_only_a_run_that_is_under_way() {
        let launch = Launch::new(None, None, None, None);
        assert!(!launch.abort(), "no run yet");
        let (kill, mut killed) = oneshot::channel();
        *launch.lock() = Some(Run {
            id: 1,
            key: "alpha.nvim-0123456789ab".into(),
            what: What::Suite,
            root: PathBuf::from("/repos/alpha.nvim"),
            running: true,
            exit: None,
            started: 1,
            ended: None,
            output: Vec::new(),
            document: Document::default(),
            kill: Some(kill),
        });
        assert!(launch.abort());
        assert!(killed.try_recv().is_ok(), "the run was told to end");
        // Asked again before the run has ended: still under way, nothing more to tell.
        assert!(launch.abort());
        launch.lock().as_mut().unwrap().running = false;
        assert!(!launch.abort(), "a run that ended");
    }

    #[test]
    fn the_exit_code_of_the_engine_is_the_verdict() {
        assert_eq!(verdict(Some(0)), "green");
        assert_eq!(verdict(Some(1)), "red");
        assert_eq!(verdict(Some(2)), "refused");
        assert_eq!(verdict(Some(3)), "failed");
        assert_eq!(verdict(None), "stopped");
    }

    /// A line of the engine as `sheets run --result` prints it.
    fn result_line(red: bool, written: Value, tasks: Value) -> String {
        json!({ "schema": SHEET_RUN_SCHEMA, "exit": 1, "task": "a/b", "sheets": [], "red": red,
                "tasks": tasks, "written": written })
        .to_string()
    }

    #[test]
    fn stdout_holds_the_document_until_it_is_whole_and_never_shows_it() {
        let line = format!(
            "{{\"schema\":\"{SHEET_RUN_SCHEMA}\",\"red\":false,\"written\":true,\"tasks\":[]}}\r\n"
        );
        let text = format!("sheet a: pass\n{line}{{\"other\":1}}\nlast");
        // In pieces of every size, split anywhere: the same lines, the same document.
        for size in [1, 2, 7, 64, text.len()] {
            let mut doc = Document::default();
            let mut out = Vec::new();
            for piece in text.as_bytes().chunks(size) {
                doc.feed(piece, &mut out);
            }
            doc.finish(&mut out, false);
            assert_eq!(
                String::from_utf8(out).unwrap(),
                "sheet a: pass\n{\"other\":1}\nlast",
                "{size}"
            );
            assert_eq!(doc.doc.as_ref().unwrap()["written"], true, "{size}");
        }
        // A document that is cut off at the end of stdout is none, and is not shown either.
        let mut doc = Document::default();
        let mut out = Vec::new();
        doc.feed(
            b"text
", &mut out,
        );
        doc.feed(&line.as_bytes()[..20], &mut out);
        doc.finish(&mut out, false);
        assert!(doc.doc.is_none());
        assert_eq!(
            out,
            b"text
"
        );
        // One too large is dropped whole: no document, nothing of it in the output.
        let big = format!(
            "{{\"schema\":\"{SHEET_RUN_SCHEMA}\",\"m\":\"{}\"}}\nafter\n",
            "y".repeat(MAX_DOCUMENT_BYTES)
        );
        let mut doc = Document::default();
        let mut out = Vec::new();
        for piece in big.as_bytes().chunks(8192) {
            doc.feed(piece, &mut out);
        }
        assert!(doc.doc.is_none());
        assert_eq!(out, b"after\n");
    }

    #[test]
    fn the_engines_document_of_a_run_is_its_last_line_of_that_schema() {
        let line = result_line(
            false,
            json!(true),
            json!([{ "id": "a/b", "outcome": "changed" }]),
        );
        let out = format!("sheet try: pass\ntesting sheets: task a/b changed\n{line}\r\n");
        assert_eq!(sheet_result(&out).unwrap()["written"], true);
        // A line of stderr after it does not hide it; the last document counts.
        let later = result_line(true, json!(null), json!([]));
        let two = format!("{line}\n{later}\nnote: a line of stderr\n");
        assert_eq!(sheet_result(&two).unwrap()["red"], true);
        // No document: lines for a person, JSON of another schema, a higher schema, what is no JSON.
        for no in [
            "",
            "testing sheets: task a/b changed",
            r#"{"schema":"testing-sheet-run/2","red":false,"written":false}"#,
            r#"{"schema":"testing-export/1"}"#,
            r#"{"schema":"testing-sheet-run/1""#,
            r#"x {"schema":"testing-sheet-run/1"}"#,
        ] {
            assert_eq!(sheet_result(no), None, "{no}");
        }
    }

    #[test]
    fn a_task_that_was_not_written_is_not_a_red_sheet() {
        let doc = |red: bool, written: Value| -> Value {
            serde_json::from_str(&result_line(red, written, json!([]))).unwrap()
        };
        // Exit 1 is both; the engine's document tells them apart.
        assert_eq!(
            verdict_of(Some(1), Some(&doc(false, json!(false)))),
            "not_written"
        );
        assert_eq!(verdict_of(Some(1), Some(&doc(true, json!(false)))), "red");
        assert_eq!(verdict_of(Some(1), Some(&doc(true, json!(true)))), "red");
        // Without a document, or with one that does not say: the exit code alone, as before.
        assert_eq!(verdict_of(Some(1), None), "red");
        assert_eq!(verdict_of(Some(1), Some(&doc(false, json!(null)))), "red");
        assert_eq!(
            verdict_of(Some(1), Some(&json!({ "schema": SHEET_RUN_SCHEMA }))),
            "red"
        );
        // No document turns another exit code into this verdict.
        let unwritten = doc(false, json!(false));
        assert_eq!(verdict_of(Some(0), Some(&unwritten)), "green");
        assert_eq!(verdict_of(Some(3), Some(&unwritten)), "failed");
        assert_eq!(verdict_of(None, Some(&unwritten)), "stopped");
    }

    #[test]
    fn the_block_of_a_task_is_what_the_engine_said_of_that_task() {
        let doc = |written: Value, tasks: Value| -> Value {
            serde_json::from_str(&result_line(false, written, tasks)).unwrap()
        };
        let failed = doc(
            json!(false),
            json!([{ "id": "x/y", "outcome": "changed" },
                   { "id": "a/b", "outcome": "failed", "message": "conflict: C:/vault/x.md" }]),
        );
        assert_eq!(
            block_of(Some(&failed), "a/b"),
            Some(json!({ "written": false, "outcome": "failed" }))
        );
        // Nothing was to be written, or no document: no block.
        assert_eq!(block_of(Some(&doc(json!(null), json!([]))), "a/b"), None);
        assert_eq!(block_of(None, "a/b"), None);
        // tasks.nvim did not answer: not written, with a word of the hub's own.
        assert_eq!(
            block_of(Some(&doc(json!(false), json!([]))), "a/b"),
            Some(json!({ "written": false, "outcome": "unreachable" }))
        );
        // A word that is none is not passed on.
        let odd = doc(
            json!(false),
            json!([{ "id": "a/b", "outcome": "<b>x</b>" }]),
        );
        assert_eq!(
            block_of(Some(&odd), "a/b"),
            Some(json!({ "written": false, "outcome": "unreachable" }))
        );
    }

    #[test]
    fn a_device_sees_a_task_run_without_its_output_and_of_any_other_only_that_it_runs() {
        let launch = Launch::new(None, None, None, None);
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
                document: Document::default(),
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
        // The engine said nothing of the task's block: the state says so, a device hears the same.
        assert_eq!(device["run"]["block"], Value::Null);
        assert_eq!(device["run"]["sheets_red"], Value::Null);
        // A line for a person is not read any more: only the engine's document counts.
        let person =
            "sheet try: pass\ntesting sheets: task alpha.nvim/try failed: conflict: /vault/x.md\n";
        launch.lock().as_mut().unwrap().output = person.as_bytes().to_vec();
        assert_eq!(launch.state_for_device()["run"]["block"], Value::Null);
        assert_eq!(launch.state_for_device()["run"]["verdict"], "red");
        // The document: green sheets, the task not written. The verdict says so, and no text of the engine
        // (its message may name a file of the vault) reaches a device.
        let doc = json!({ "schema": SHEET_RUN_SCHEMA, "exit": 1, "task": "alpha.nvim/try", "red": false,
            "sheets": [{ "id": "try", "status": "pass" }], "written": false,
            "tasks": [{ "id": "alpha.nvim/try", "outcome": "failed", "message": "conflict: /vault/x.md" }] });
        {
            let mut guard = launch.lock();
            let run = guard.as_mut().unwrap();
            let mut out = Vec::new();
            run.document
                .feed(format!("{person}{doc}\n").as_bytes(), &mut out);
            run.output = out;
        }
        let device = launch.state_for_device();
        assert_eq!(device["run"]["verdict"], "not_written", "{device}");
        assert_eq!(
            device["run"]["block"],
            json!({ "written": false, "outcome": "failed" })
        );
        assert_eq!(device["run"]["sheets_red"], false);
        assert!(!device.to_string().contains("/vault"), "{device}");
    }
}
