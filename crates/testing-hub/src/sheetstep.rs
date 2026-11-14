//! A test sheet step by step, from the page: `testing sheets step <sheet> --root <root>` as a child of the hub,
//! the first thing this hub starts that is not a question. The child speaks `testing-sheet-session/1`: one JSON
//! event per line on its stdout, one command per line on its stdin, and a closed stdin is `abort`.
//!
//! What holds here, each with a test:
//!
//! * one run per hub: a second start while one runs is refused and names the sheet that runs;
//! * the child is started the way the engine is (the same Neovim, the same script, the allow-listed environment
//!   plus the trust and the machine role of the person, which `sheets step` reads itself), with the root of a
//!   repository that the engine named for a key of the fleet: never a path of a request;
//! * a command is one of a closed list, built here; the text of an answer or a note is one line of bounded length;
//! * the page sees no path: `root` and `path` of `start`, `image` and `shot` of `step-after` are taken out, a picture
//!   is asked for by its step, and the hub serves only the file the child named for that step;
//! * the events are kept for a page that comes later (`start`, every `size`, every `step-after`, the last stop, the
//!   `end`) and passed on by the event stream of the hub as they come;
//! * a child that writes nothing holds no request: only `start` waits, for the first event or the end, and not long;
//! * `abort` and the stop of the hub say `abort` to the child and close its stdin, so that it takes its own target
//!   down; a child that does not end on that within a grace is killed.
//!
//! The workstation rule and the trust of a root are the engine's own (`testing sheets step` refuses with exit 2 and
//! the reason on stderr): a refusal before the first event is the answer of `start`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use appkit_hub::discovery::{engine_script, Setup};
use appkit_hub::engine::{child_env, is_unprintable, EngineVars};
use appkit_hub::env::Env;
use serde_json::{json, Map, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::engine::{DIALECT, DISCOVERY};

/// The contract the child speaks; a line of another schema ends the run.
pub const SCHEMA: &str = "testing-sheet-session/1";

/// The name of the event of the stream that carries a line of the child.
pub const EVENT: &str = "sheet-step";

/// The longest text of an answer (`answer no <text>`), in characters: one line, nothing unprintable.
pub const MAX_ANSWER_CHARS: usize = 400;

/// The longest note (`note <text>`), in bytes: the limit of `testing sheets step`.
pub const MAX_NOTE_BYTES: usize = 1000;

/// The highest step number a command may name.
pub const MAX_STEP: u64 = 100_000;

/// The longest line the child may write: a `screen` of a big editor is far below it; a child that writes more has
/// gone astray and is ended.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

/// What is kept of the child's stderr, for the reason of a refusal.
const MAX_STDERR_BYTES: usize = 64 * 1024;

/// How long `start` waits for the child's first event or its end: a refusal is answered, not streamed.
pub const START_WAIT: Duration = Duration::from_secs(20);

/// Between telling the child `abort` and killing a child that did not end on it: it takes its own target (an
/// editor, a browser) down first, which takes a second, and a few more on a loaded machine.
pub const GRACE: Duration = Duration::from_secs(10);

/// How long the last lines of a child that ended are read. A process it left behind (a target that did not go down)
/// may hold its stdout and stderr for as long as it lives; that does not hold the run.
const DRAIN: Duration = Duration::from_secs(2);

/// The largest picture that is served.
pub const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

/// The longest name of a sheet (an id or a file name below `TESTS/sheets`).
pub const MAX_SHEET_NAME: usize = 100;

/// The most events a run may leave for a page that comes later; beyond it the child has gone astray.
const MAX_REPLAY: usize = 20_000;

/// The variables of the person's environment that `sheets step` reads itself and gets beyond the allow-list of
/// the engine: the trust of roots and the rule of the workstation (the child refuses where a terminal would),
/// where Neovim keeps the pictures of a run (`stdpath("cache")`), and `SYSTEMDRIVE`, without which Neovim on
/// Windows takes ten seconds to end the child editor of a sheet (measured: an abort then outlives the grace). A
/// browser or desktop sheet finds its driver through the `TESTING_*` and Playwright variables (without them it is a
/// reported skip that a terminal would not have), and the program a sheet names may be built with `cargo`.
const PASS_THROUGH: &[&str] = &[
    "TESTING_TRUST_ROOTS",
    "NVIM_MACHINE_ROLE",
    "TESTING_ALLOW_WORKSTATION",
    "TESTING_PLAYWRIGHT_DIR",
    "TESTING_DESKTOP_DRIVER",
    "PLAYWRIGHT_BROWSERS_PATH",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "SYSTEMDRIVE",
    "LOCALAPPDATA",
    "APPDATA",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
    "XDG_DATA_HOME",
];

/// How the child is started: the program, what comes before `sheets step <sheet> --root <root>`, its environment
/// and its working folder. The program is Neovim with the script of the engine; a test gives a script of its own.
#[derive(Debug, Clone)]
pub struct StepProgram {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub cwd: Option<PathBuf>,
}

impl StepProgram {
    /// The program for what the search for the engine found: as `CliEngine` starts `testing call`.
    #[must_use]
    pub fn from_setup(setup: &Setup, env: &Env) -> Self {
        let script = engine_script(&DISCOVERY, &setup.engine_dir.value);
        let vars = EngineVars {
            dialect: DIALECT,
            vault: &setup.vault.value,
            lib_dir: setup.lib_dir.as_ref().map(|l| l.value.as_path()),
            extra_areas: &setup.extra_areas,
        };
        let mut vars_env = child_env(env, Some(&vars));
        vars_env.extend(passed_through(env));
        Self {
            program: setup.nvim.value.clone(),
            args: ["--headless", "-u", "NONE", "-i", "NONE", "-n", "-l"]
                .iter()
                .map(OsString::from)
                .chain(std::iter::once(script.into_os_string()))
                .collect(),
            env: vars_env,
            cwd: Some(setup.engine_dir.value.clone()),
        }
    }
}

/// A command to the run: the closed list of `testing-sheet-session/1`, nothing else reaches the child's stdin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Next,
    Continue,
    Until(u64),
    Break(u64),
    Clear(u64),
    Answer {
        yes: bool,
        text: Option<String>,
    },
    /// A note on the step where the run stands; taken at once, also while a step runs.
    Note(String),
    /// The request an AI would get about a step (one of [`EXPLAIN_KINDS`]): the child answers with an `explain`
    /// event for the page to show. Nothing is sent anywhere, by the child or by the hub.
    Explain(u64, &'static str),
    Abort,
}

/// The questions about a step a request can be built for, as testing.nvim names them (`testing.sheet.explain`): what
/// the step does, what to watch out for, what its result means.
pub const EXPLAIN_KINDS: [&str; 3] = ["what", "watch", "result"];

/// The `text` of a command, trimmed: one line without control characters. `None` when it is missing, null or empty.
fn line_text(map: &Map<String, Value>) -> Result<Option<String>, String> {
    match map.get("text") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => {
            let text = text.trim();
            if text.chars().any(is_unprintable) {
                return Err("text must be one line without control characters".to_owned());
            }
            Ok((!text.is_empty()).then(|| text.to_owned()))
        }
        Some(_) => Err("text must be a string".to_owned()),
    }
}

impl Command {
    /// The command of a request body `{ "command": "<name>", "n"?: <step>, "yes"?: <bool>, "text"?: "<line>",
    /// "kind"?: "<question>" }`.
    ///
    /// # Errors
    ///
    /// Why the body is no command, in a line for the page.
    pub fn parse(body: &Value) -> Result<Self, String> {
        let map = body
            .as_object()
            .ok_or_else(|| "the body must be a JSON object".to_owned())?;
        for key in map.keys() {
            if !["command", "n", "yes", "text", "kind"].contains(&key.as_str()) {
                return Err(format!("unknown field {key:?}"));
            }
        }
        let name = map
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| "command must be a word".to_owned())?;
        let step = || -> Result<u64, String> {
            let n = map
                .get("n")
                .and_then(Value::as_u64)
                .filter(|n| (1..=MAX_STEP).contains(n))
                .ok_or_else(|| format!("{name} needs n, a step number from 1 to {MAX_STEP}"))?;
            Ok(n)
        };
        let no_extra = |allowed: &[&str]| -> Result<(), String> {
            for key in map.keys() {
                if key != "command" && !allowed.contains(&key.as_str()) {
                    return Err(format!("{name} takes no {key}"));
                }
            }
            Ok(())
        };
        match name {
            "next" => no_extra(&[]).map(|()| Self::Next),
            "continue" => no_extra(&[]).map(|()| Self::Continue),
            "abort" => no_extra(&[]).map(|()| Self::Abort),
            "until" => no_extra(&["n"]).and_then(|()| step().map(Self::Until)),
            "break" => no_extra(&["n"]).and_then(|()| step().map(Self::Break)),
            "clear" => no_extra(&["n"]).and_then(|()| step().map(Self::Clear)),
            "answer" => {
                no_extra(&["yes", "text"])?;
                let yes = map
                    .get("yes")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| "answer needs yes, true or false".to_owned())?;
                let text = line_text(map)?;
                if text
                    .as_ref()
                    .is_some_and(|t| t.chars().count() > MAX_ANSWER_CHARS)
                {
                    return Err(format!("text is longer than {MAX_ANSWER_CHARS} characters"));
                }
                Ok(Self::Answer { yes, text })
            }
            "note" => {
                no_extra(&["text"])?;
                let text = line_text(map)?.ok_or_else(|| "note needs a text".to_owned())?;
                if text.len() > MAX_NOTE_BYTES {
                    return Err(format!("text is longer than {MAX_NOTE_BYTES} bytes"));
                }
                Ok(Self::Note(text))
            }
            "explain" => {
                no_extra(&["n", "kind"])?;
                let n = step()?;
                let kind = map
                    .get("kind")
                    .and_then(Value::as_str)
                    .and_then(|kind| EXPLAIN_KINDS.iter().find(|word| **word == kind))
                    .ok_or_else(|| "explain needs kind: what, watch or result".to_owned())?;
                Ok(Self::Explain(n, kind))
            }
            other => Err(format!("unknown command {other:?}")),
        }
    }

    /// The line on the child's stdin, with its end.
    #[must_use]
    pub fn line(&self) -> String {
        match self {
            Self::Next => "next\n".to_owned(),
            Self::Continue => "continue\n".to_owned(),
            Self::Abort => "abort\n".to_owned(),
            Self::Until(n) => format!("until {n}\n"),
            Self::Break(n) => format!("break {n}\n"),
            Self::Clear(n) => format!("clear {n}\n"),
            Self::Answer { yes, text } => {
                let word = if *yes { "yes" } else { "no" };
                match text {
                    Some(text) => format!("answer {word} {text}\n"),
                    None => format!("answer {word}\n"),
                }
            }
            Self::Note(text) => format!("note {text}\n"),
            Self::Explain(n, kind) => format!("explain {n} {kind}\n"),
        }
    }
}

/// Is this the name of a sheet the child may be given: an id or a file name below `TESTS/sheets`, nothing that
/// could name a folder, an option or another file.
#[must_use]
pub fn is_sheet_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_SHEET_NAME
        && !name.starts_with(['.', '-'])
        && !name.contains("..")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// Is this a size as the child names one: `80x24`.
#[must_use]
pub fn is_size(size: &str) -> bool {
    size.len() <= 11
        && size
            .split_once('x')
            .is_some_and(|(cols, rows)| is_number(cols) && is_number(rows))
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.len() <= 5 && text.bytes().all(|b| b.is_ascii_digit())
}

/// The sheets of a repository: the names of the files below `TESTS/sheets` without their `.sheet.lua`, sorted.
/// A repository without the folder has none.
///
/// # Errors
///
/// The folder is there but cannot be read.
pub fn list_sheets(root: &Path) -> std::io::Result<Vec<String>> {
    let dir = root.join("TESTS").join("sheets");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let base = name.strip_suffix(".sheet.lua")?;
            is_sheet_name(base).then(|| base.to_owned())
        })
        .collect();
    names.sort();
    Ok(names)
}

/// The pictures a step left: the files the child named, by the step.
#[derive(Debug, Clone, Default)]
struct Pictures {
    image: Option<PathBuf>,
    shot: Option<PathBuf>,
}

/// What a page that comes later is told, in order: the whole sheet, the sizes, what every step did, the last stop,
/// the end.
#[derive(Debug, Default)]
struct Replay {
    start: Option<Value>,
    sizes: Vec<Value>,
    steps: BTreeMap<String, BTreeMap<u64, Value>>,
    /// The last `step-before`, `paused` or `ask`: where the run stands.
    current: Option<Value>,
    /// Every `note`, in order.
    notes: Vec<Value>,
    end: Option<Value>,
    /// The hub's own note when the child ended without an `end`.
    exit: Option<Value>,
    count: usize,
}

impl Replay {
    /// A breakpoint set or cleared during the run: the child announces none, and a page that comes later reads the
    /// breakpoints from `start`.
    fn set_break(&mut self, n: u64, on: bool) {
        let Some(breaks) = self
            .start
            .as_mut()
            .and_then(|start| start.get_mut("breaks"))
            .and_then(Value::as_array_mut)
        else {
            return;
        };
        breaks.retain(|b| b.as_u64() != Some(n));
        if on {
            breaks.push(Value::from(n));
            breaks.sort_by_key(|b| b.as_u64().unwrap_or(0));
        }
    }

    fn events(&self) -> Vec<Value> {
        let mut out = Vec::new();
        out.extend(self.start.iter().cloned());
        out.extend(self.sizes.iter().cloned());
        for marks in self.steps.values() {
            out.extend(marks.values().cloned());
        }
        out.extend(self.notes.iter().cloned());
        out.extend(self.current.iter().cloned());
        out.extend(self.end.iter().cloned());
        out.extend(self.exit.iter().cloned());
        out
    }
}

struct Session {
    id: u64,
    key: String,
    sheet: String,
    root: PathBuf,
    /// The lines for the child's stdin; dropping it closes the stdin.
    stdin: Option<mpsc::UnboundedSender<String>>,
    /// Ends the child at once.
    kill: Option<oneshot::Sender<()>>,
    running: bool,
    started: bool,
    seq: u64,
    replay: Replay,
    pictures: BTreeMap<(String, u64), Pictures>,
    stderr: String,
    /// Why the hub ended the child, for the page.
    astray: Option<String>,
}

/// Why a run did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// This hub has no program to run a sheet with (no engine).
    Unavailable,
    /// A run is under way, of this sheet.
    Busy { sheet: String },
    /// The child could not be started.
    Spawn(String),
    /// The child ended before its first event: refused (exit 2, the reason), or broken.
    Refused { code: Option<i32>, detail: String },
}

/// Why a command was not passed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    NotRunning,
}

/// The one run of this hub, and what the page may ask of it.
pub struct SheetStep {
    program: Option<StepProgram>,
    inner: Mutex<Option<Session>>,
    /// The lines of the child, as the event stream sends them.
    tx: broadcast::Sender<Arc<str>>,
    /// Wakes `start` on the first event or the end of the child.
    wake: watch::Sender<u64>,
    counter: AtomicU64,
}

impl SheetStep {
    /// With the program; `None`: no run can start, the state says so.
    #[must_use]
    pub fn new(program: Option<StepProgram>) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            program,
            inner: Mutex::new(None),
            tx,
            wake: watch::Sender::new(0),
            counter: AtomicU64::new(0),
        }
    }

    /// Whether this hub can run a sheet at all.
    #[must_use]
    pub fn available(&self) -> bool {
        self.program.is_some()
    }

    /// The lines of the child from now on, for an event stream.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<str>> {
        self.tx.subscribe()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Session>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Where the run stands, for a page that reads before it follows the stream.
    #[must_use]
    pub fn state(&self) -> Value {
        let guard = self.lock();
        match guard.as_ref() {
            None => json!({
                "schema": 1,
                "kind": "hub.sheet_step",
                "available": self.available(),
                "session": null,
                "running": false,
                "seq": 0,
                "key": null,
                "sheet": null,
                "events": [],
            }),
            Some(s) => json!({
                "schema": 1,
                "kind": "hub.sheet_step",
                "available": self.available(),
                "session": s.id,
                "running": s.running,
                "seq": s.seq,
                "key": s.key,
                "sheet": s.sheet,
                "events": s.replay.events(),
            }),
        }
    }

    /// Start a run of `sheet` in `root`, the repository with the key `key`. Waits for the child's first event or its
    /// end, at most `START_WAIT`; the state of the run is the answer.
    ///
    /// # Errors
    ///
    /// [`StartError`]: no program, a run under way, no child, or a child that ended before its first event.
    pub async fn start(
        self: &Arc<Self>,
        key: &str,
        sheet: &str,
        root: &Path,
    ) -> Result<Value, StartError> {
        let Some(program) = &self.program else {
            return Err(StartError::Unavailable);
        };
        let (stdin_tx, stdin_rx) = mpsc::unbounded_channel::<String>();
        let (kill_tx, kill_rx) = oneshot::channel::<()>();
        let id;
        {
            let mut guard = self.lock();
            if let Some(s) = guard.as_ref().filter(|s| s.running) {
                return Err(StartError::Busy {
                    sheet: s.sheet.clone(),
                });
            }
            id = self.counter.fetch_add(1, Ordering::AcqRel) + 1;
            let mut child = tokio::process::Command::new(&program.program);
            child
                .args(&program.args)
                .arg("sheets")
                .arg("step")
                .arg(sheet)
                .arg("--root")
                .arg(root)
                .env_clear()
                .envs(program.env.iter().map(|(k, v)| (k, v)))
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true);
            if let Some(cwd) = &program.cwd {
                child.current_dir(cwd);
            }
            #[cfg(windows)]
            {
                // CREATE_NO_WINDOW: no console window for a process the hub starts, as for its engine.
                child.creation_flags(0x0800_0000);
            }
            let mut child = match child.spawn() {
                Ok(child) => child,
                Err(e) => return Err(StartError::Spawn(scrub(&e.to_string(), &[root]))),
            };
            let stdin = child.stdin.take();
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            *guard = Some(Session {
                id,
                key: key.to_owned(),
                sheet: sheet.to_owned(),
                root: root.to_path_buf(),
                stdin: Some(stdin_tx),
                kill: Some(kill_tx),
                running: true,
                started: false,
                seq: 0,
                replay: Replay::default(),
                pictures: BTreeMap::new(),
                stderr: String::new(),
                astray: None,
            });
            drop(guard);
            self.spawn_tasks(id, child, stdin, stdout, stderr, stdin_rx, kill_rx);
        }
        // The first event or the end, whichever comes first; a child that takes longer is followed by the stream.
        let mut woken = self.wake.subscribe();
        let _ = tokio::time::timeout(START_WAIT, async {
            loop {
                let (started, running) = {
                    let guard = self.lock();
                    match guard.as_ref().filter(|s| s.id == id) {
                        Some(s) => (s.started, s.running),
                        None => (false, false),
                    }
                };
                if started || !running {
                    return;
                }
                if woken.changed().await.is_err() {
                    return;
                }
            }
        })
        .await;
        let mut guard = self.lock();
        let Some(s) = guard.as_mut().filter(|s| s.id == id) else {
            return Err(StartError::Spawn("the run is gone".to_owned()));
        };
        if !s.started && !s.running {
            let code = s.replay.exit.as_ref().and_then(|e| e["code"].as_i64());
            let detail = s
                .replay
                .exit
                .as_ref()
                .and_then(|e| e["detail"].as_str())
                .unwrap_or("")
                .to_owned();
            *guard = None;
            return Err(StartError::Refused {
                code: code.and_then(|c| i32::try_from(c).ok()),
                detail,
            });
        }
        drop(guard);
        Ok(self.state())
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_tasks(
        self: &Arc<Self>,
        id: u64,
        mut child: tokio::process::Child,
        stdin: Option<tokio::process::ChildStdin>,
        stdout: Option<tokio::process::ChildStdout>,
        stderr: Option<tokio::process::ChildStderr>,
        mut stdin_rx: mpsc::UnboundedReceiver<String>,
        kill_rx: oneshot::Receiver<()>,
    ) {
        // The writer: a line at a time; when the sender is gone the stdin closes, which is `abort` to the child.
        if let Some(mut stdin) = stdin {
            tokio::spawn(async move {
                while let Some(line) = stdin_rx.recv().await {
                    if stdin.write_all(line.as_bytes()).await.is_err()
                        || stdin.flush().await.is_err()
                    {
                        break;
                    }
                }
                drop(stdin);
            });
        }
        let me = Arc::clone(self);
        let mut reader = tokio::spawn(async move {
            let Some(stdout) = stdout else {
                return;
            };
            let mut lines = BufReader::new(stdout);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                let read = (&mut lines)
                    .take(MAX_LINE_BYTES as u64 + 1)
                    .read_until(b'\n', &mut buf)
                    .await;
                match read {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
                if buf.len() > MAX_LINE_BYTES {
                    me.fail(id, "a line of the child is longer than the limit");
                    return;
                }
                let line = String::from_utf8_lossy(&buf);
                me.on_line(id, line.trim_end_matches(['\r', '\n']));
            }
        });
        let me = Arc::clone(self);
        let mut errs = tokio::spawn(async move {
            let Some(mut stderr) = stderr else {
                return;
            };
            let mut buf = Vec::new();
            let _ = (&mut stderr)
                .take(MAX_STDERR_BYTES as u64)
                .read_to_end(&mut buf)
                .await;
            let text = String::from_utf8_lossy(&buf).into_owned();
            let mut guard = me.lock();
            if let Some(s) = guard.as_mut().filter(|s| s.id == id) {
                s.stderr = text;
            }
        });
        let me = Arc::clone(self);
        tokio::spawn(async move {
            let code = tokio::select! {
                status = child.wait() => status.ok().and_then(|s| s.code()),
                _ = kill_rx => {
                    let _ = child.kill().await;
                    None
                }
            };
            let drained = tokio::time::timeout(DRAIN, async {
                let _ = (&mut reader).await;
                let _ = (&mut errs).await;
            })
            .await;
            if drained.is_err() {
                reader.abort();
                errs.abort();
            }
            me.on_exit(id, code);
        });
    }

    /// One line of the child.
    fn on_line(&self, id: u64, line: &str) {
        let Ok(Value::Object(mut event)) = serde_json::from_str::<Value>(line) else {
            // Not an event: a stray line of the child, left alone.
            return;
        };
        if event.get("schema").and_then(Value::as_str) != Some(SCHEMA) {
            self.fail(id, "the child speaks another schema");
            return;
        }
        let Some(name) = event
            .get("event")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return;
        };
        let mut guard = self.lock();
        let Some(s) = guard.as_mut().filter(|s| s.id == id) else {
            return;
        };
        // What names a path of this machine stays here.
        match name.as_str() {
            "start" => {
                event.remove("root");
                event.remove("path");
            }
            "step-after" => {
                let size = event
                    .get("size")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let n = event.get("n").and_then(Value::as_u64).unwrap_or(0);
                let image = take_path(&mut event, "image");
                let shot = take_path(&mut event, "shot");
                if image.is_some() || shot.is_some() {
                    s.pictures.insert((size, n), Pictures { image, shot });
                }
            }
            _ => {}
        }
        let mut value = Value::Object(event);
        hide_root(&mut value, &s.root);
        s.replay.count += 1;
        if s.replay.count > MAX_REPLAY {
            drop(guard);
            self.fail(id, "the child writes more events than a sheet can have");
            return;
        }
        match name.as_str() {
            "start" => {
                s.started = true;
                s.replay.start = Some(value.clone());
            }
            "size" => s.replay.sizes.push(value.clone()),
            "step-after" => {
                let size = value["size"].as_str().unwrap_or("").to_owned();
                let n = value["n"].as_u64().unwrap_or(0);
                s.replay
                    .steps
                    .entry(size)
                    .or_default()
                    .insert(n, value.clone());
                // The question of this step is answered.
                if s.replay
                    .current
                    .as_ref()
                    .is_some_and(|c| c["event"] == "ask")
                {
                    s.replay.current = None;
                }
            }
            "step-before" | "paused" | "ask" => s.replay.current = Some(value.clone()),
            "note" => s.replay.notes.push(value.clone()),
            "end" => {
                s.replay.current = None;
                s.replay.end = Some(value.clone());
            }
            // `task`, `error` and `explain` (the answer to a question of the page) are of the moment; an unknown
            // event is for a newer reader.
            _ => {}
        }
        s.seq += 1;
        let seq = s.seq;
        let line = sse_line(id, seq, &value);
        drop(guard);
        self.wake.send_replace(seq);
        let _ = self.tx.send(line);
    }

    /// The child has gone astray: it is ended, and the page is told why.
    fn fail(&self, id: u64, why: &str) {
        let kill = {
            let mut guard = self.lock();
            match guard.as_mut().filter(|s| s.id == id && s.running) {
                Some(s) => {
                    s.stdin = None;
                    s.astray = Some(why.to_owned());
                    s.kill.take()
                }
                None => None,
            }
        };
        if let Some(kill) = kill {
            eprintln!("sheet step: {why}");
            let _ = kill.send(());
        }
    }

    /// The child ended: free the run, and tell the page when there was no `end`.
    fn on_exit(&self, id: u64, code: Option<i32>) {
        let mut guard = self.lock();
        let Some(s) = guard.as_mut().filter(|s| s.id == id) else {
            return;
        };
        s.running = false;
        s.stdin = None;
        s.kill = None;
        let mut line = None;
        if s.replay.end.is_none() {
            let detail = s
                .astray
                .clone()
                .unwrap_or_else(|| scrub(&s.stderr, &[&s.root]));
            let value = json!({ "event": "exit", "code": code, "detail": detail });
            s.replay.exit = Some(value.clone());
            s.replay.current = None;
            s.seq += 1;
            line = Some(sse_line(id, s.seq, &value));
        }
        let seq = s.seq;
        drop(guard);
        self.wake.send_replace(seq);
        if let Some(line) = line {
            let _ = self.tx.send(line);
        }
    }

    /// Pass a command to the run.
    ///
    /// # Errors
    ///
    /// No run is under way.
    pub fn command(self: &Arc<Self>, command: &Command) -> Result<(), CommandError> {
        if *command == Command::Abort {
            self.abort();
            return Ok(());
        }
        let mut guard = self.lock();
        let Some(s) = guard.as_mut().filter(|s| s.running) else {
            return Err(CommandError::NotRunning);
        };
        match command {
            Command::Break(n) => s.replay.set_break(*n, true),
            Command::Clear(n) => s.replay.set_break(*n, false),
            _ => {}
        }
        if let Some(stdin) = &s.stdin {
            // A writer that is gone is a child that is ending: nothing to say to it.
            let _ = stdin.send(command.line());
        }
        Ok(())
    }

    /// End the run: the child gets `abort` and its stdin closes (both mean the same to it; it then takes its own
    /// target down and says `end`); a child that does not end by itself within the grace is killed. Nothing when no
    /// run is under way.
    pub fn abort(self: &Arc<Self>) {
        let id = {
            let mut guard = self.lock();
            match guard.as_mut().filter(|s| s.running) {
                Some(s) => {
                    if let Some(stdin) = s.stdin.take() {
                        let _ = stdin.send(Command::Abort.line());
                    }
                    s.id
                }
                None => return,
            }
        };
        let me = Arc::clone(self);
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    tokio::time::sleep(GRACE).await;
                    me.kill(id);
                });
            }
            // Outside a runtime nothing could wait: the child is ended at once.
            Err(_) => me.kill(id),
        }
    }

    fn kill(&self, id: u64) {
        let kill = {
            let mut guard = self.lock();
            match guard.as_mut().filter(|s| s.id == id && s.running) {
                Some(s) => s.kill.take(),
                None => None,
            }
        };
        if let Some(kill) = kill {
            let _ = kill.send(());
        }
    }

    /// The hub stops: the run is ended as by `abort`, so that the child takes its own target (an editor, a browser)
    /// down with it; the grace is shorter than the hub's own, and a child that ignores it is killed. Nothing stays
    /// behind of the hub itself: the child is killed with the hub's last task too (`kill_on_drop`).
    pub fn stop(self: &Arc<Self>) {
        self.abort();
    }

    /// The file of a picture the child named for this step of this size (`kind` is `image` or `shot`), from the run
    /// that is or was last. Only what the child reported: a request names a step, never a file.
    #[must_use]
    pub fn picture(&self, size: &str, n: u64, kind: &str) -> Option<PathBuf> {
        let guard = self.lock();
        let s = guard.as_ref()?;
        let pictures = s.pictures.get(&(size.to_owned(), n))?;
        match kind {
            "image" => pictures.image.clone(),
            "shot" => pictures.shot.clone(),
            _ => None,
        }
    }
}

/// A path the child named, taken out of the event; what is left says only that there is one.
fn take_path(event: &mut Map<String, Value>, field: &str) -> Option<PathBuf> {
    let path = event.remove(field)?;
    let text = path.as_str().filter(|t| !t.is_empty())?;
    event.insert(field.to_owned(), Value::Bool(true));
    Some(PathBuf::from(text))
}

/// The variables of [`PASS_THROUGH`] the person's environment holds.
fn passed_through(env: &Env) -> Vec<(OsString, OsString)> {
    PASS_THROUGH
        .iter()
        .filter_map(|name| env.get(name).map(|v| (OsString::from(name), v.clone())))
        .collect()
}

/// `root` in a text becomes `<path>`, in both ways of writing it.
fn hide_in_text(text: &str, root: &Path) -> String {
    let shown = root.display().to_string();
    if shown.len() <= 3 {
        return text.to_owned();
    }
    text.replace(&shown, "<path>")
        .replace(&shown.replace('\\', "/"), "<path>")
}

/// `root` becomes `<path>` in every text of a value: the lines of the engine (a failure, a snapshot, a reason) may
/// name a file of the run in their wording.
pub(crate) fn hide_root(value: &mut Value, root: &Path) {
    match value {
        Value::String(text) => {
            let hidden = hide_in_text(text, root);
            if hidden != *text {
                *text = hidden;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| hide_root(item, root)),
        Value::Object(map) => map.values_mut().for_each(|item| hide_root(item, root)),
        _ => {}
    }
}

fn sse_line(session: u64, seq: u64, event: &Value) -> Arc<str> {
    let data = json!({ "session": session, "seq": seq, "event": event });
    Arc::from(format!("event: {EVENT}\ndata: {data}\n\n"))
}

/// Text of the child's stderr for the page: the folders of this machine that are known here become `<path>`, what
/// is unprintable a blank, and it is cut to a few lines.
#[must_use]
pub fn scrub(text: &str, roots: &[&Path]) -> String {
    let mut out = text.to_owned();
    for root in roots {
        out = hide_in_text(&out, root);
    }
    let cleaned: String = out
        .chars()
        .map(|c| {
            if c == '\n' || !is_unprintable(c) {
                c
            } else {
                ' '
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    let mut short: String = trimmed.chars().take(600).collect();
    if trimmed.chars().count() > 600 {
        short.push('…');
    }
    short
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_engines_of_a_sheet_read_reaches_the_child_and_nothing_else_does() {
        let env = Env::from_pairs([
            ("TESTING_PLAYWRIGHT_DIR", "playwright"),
            ("TESTING_DESKTOP_DRIVER", "driver"),
            ("PLAYWRIGHT_BROWSERS_PATH", "browsers"),
            ("CARGO_HOME", "cargo"),
            ("RUSTUP_HOME", "rustup"),
            ("A_SECRET_TOKEN", "secret"),
        ]);
        let passed = passed_through(&env);
        let names: Vec<_> = passed.iter().map(|(k, _)| k.to_str().unwrap()).collect();
        for name in [
            "TESTING_PLAYWRIGHT_DIR",
            "TESTING_DESKTOP_DRIVER",
            "PLAYWRIGHT_BROWSERS_PATH",
            "CARGO_HOME",
            "RUSTUP_HOME",
        ] {
            assert!(names.contains(&name), "{name} is not passed on");
        }
        assert!(!names.contains(&"A_SECRET_TOKEN"));
    }

    #[test]
    fn a_folder_of_the_run_is_hidden_inside_every_text_of_an_event() {
        let root = Path::new("C:\\work\\repo");
        let mut event = json!({
            "reason": "no file C:/work/repo/a.txt",
            "failures": ["C:\\work\\repo\\b.txt differs"],
            "nested": { "line": "C:/work/repo" },
            "n": 3,
        });
        hide_root(&mut event, root);
        assert_eq!(
            event,
            json!({
                "reason": "no file <path>/a.txt",
                "failures": ["<path>\\b.txt differs"],
                "nested": { "line": "<path>" },
                "n": 3,
            })
        );
    }

    #[test]
    fn a_command_is_one_of_the_list_and_its_line_is_built_here() {
        let cases = [
            (json!({"command": "next"}), "next\n"),
            (json!({"command": "continue"}), "continue\n"),
            (json!({"command": "abort"}), "abort\n"),
            (json!({"command": "until", "n": 3}), "until 3\n"),
            (json!({"command": "break", "n": 1}), "break 1\n"),
            (json!({"command": "clear", "n": 12}), "clear 12\n"),
            (json!({"command": "answer", "yes": true}), "answer yes\n"),
            (
                json!({"command": "answer", "yes": false, "text": " the sound stutters "}),
                "answer no the sound stutters\n",
            ),
            (
                json!({"command": "answer", "yes": true, "text": ""}),
                "answer yes\n",
            ),
            (
                json!({"command": "note", "text": " the border flickers "}),
                "note the border flickers\n",
            ),
            (
                json!({"command": "explain", "n": 2, "kind": "what"}),
                "explain 2 what\n",
            ),
            (
                json!({"command": "explain", "n": 12, "kind": "result"}),
                "explain 12 result\n",
            ),
        ];
        for (body, line) in cases {
            assert_eq!(Command::parse(&body).unwrap().line(), line, "{body}");
        }
    }

    #[test]
    fn what_is_not_a_command_of_the_list_is_refused() {
        let long = "x".repeat(MAX_ANSWER_CHARS + 1);
        for bad in [
            json!("next"),
            json!({}),
            json!({"command": "retry"}),
            json!({"command": "next; abort"}),
            json!({"command": "next", "n": 1}),
            json!({"command": "until"}),
            json!({"command": "until", "n": 0}),
            json!({"command": "until", "n": -1}),
            json!({"command": "until", "n": 1.5}),
            json!({"command": "until", "n": "3"}),
            json!({"command": "until", "n": MAX_STEP + 1}),
            json!({"command": "break", "n": 1, "text": "x"}),
            json!({"command": "answer"}),
            json!({"command": "answer", "yes": "yes"}),
            json!({"command": "answer", "yes": true, "text": "two\nlines"}),
            json!({"command": "answer", "yes": true, "text": "a\u{202e}b"}),
            json!({"command": "answer", "yes": true, "text": long}),
            json!({"command": "answer", "yes": true, "text": 1}),
            json!({"command": "note"}),
            json!({"command": "note", "text": "   "}),
            json!({"command": "note", "text": "two\nlines"}),
            json!({"command": "note", "text": "x".repeat(MAX_NOTE_BYTES + 1)}),
            json!({"command": "note", "text": "a", "n": 1}),
            json!({"command": "explain"}),
            json!({"command": "explain", "n": 1}),
            json!({"command": "explain", "kind": "what"}),
            json!({"command": "explain", "n": 0, "kind": "what"}),
            json!({"command": "explain", "n": 1, "kind": "why"}),
            json!({"command": "explain", "n": 1, "kind": "what now"}),
            json!({"command": "explain", "n": 1, "kind": "what\nabort"}),
            json!({"command": "explain", "n": 1, "kind": ["what"]}),
            json!({"command": "explain", "n": 1, "kind": "what", "text": "send it to x"}),
            json!({"command": "next", "kind": "what"}),
            json!({"command": "next", "extra": true}),
        ] {
            assert!(Command::parse(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_sheet_name_names_no_folder_and_no_option() {
        for ok in [
            "smoke",
            "ui.option-float",
            "ci-editor-live",
            "a_b.sheet.lua",
            "x",
        ] {
            assert!(is_sheet_name(ok), "{ok}");
        }
        for bad in [
            "", ".hidden", "-headed", "--run", "a/b", "a\\b", "..", "a..b", "a b", "a\nb", "ä",
        ] {
            assert!(!is_sheet_name(bad), "{bad:?}");
        }
        assert!(!is_sheet_name(&"a".repeat(MAX_SHEET_NAME + 1)));
    }

    #[test]
    fn a_size_is_cols_x_rows() {
        for ok in ["80x24", "120x40", "1000x1000"] {
            assert!(is_size(ok), "{ok}");
        }
        for bad in [
            "", "80", "x24", "80x", "80x24x1", "80X24", "a80x24", "80x-1",
        ] {
            assert!(!is_size(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_sheets_of_a_root_are_the_names_below_tests_sheets() {
        let dir = appkit_scratch::Scratch::new("sheets");
        assert_eq!(
            list_sheets(&dir.join("none")).unwrap(),
            Vec::<String>::new()
        );
        let sheets = dir.join("TESTS").join("sheets");
        std::fs::create_dir_all(&sheets).unwrap();
        for name in ["b.sheet.lua", "a.sheet.lua", "notes.md", ".x.sheet.lua"] {
            std::fs::write(sheets.join(name), "return {}\n").unwrap();
        }
        std::fs::create_dir_all(sheets.join("out")).unwrap();
        assert_eq!(list_sheets(&dir).unwrap(), ["a", "b"]);
    }

    #[test]
    fn stderr_for_the_page_names_no_known_folder_and_nothing_unprintable() {
        let root = Path::new(if cfg!(windows) {
            "C:\\td\\repos\\alpha.nvim"
        } else {
            "/td/repos/alpha.nvim"
        });
        let text = format!(
            "testing sheets: {} is not trusted\u{7}: see docs",
            root.display()
        );
        let shown = scrub(&text, &[root]);
        assert_eq!(shown, "testing sheets: <path> is not trusted : see docs");
        assert!(scrub(&"y".repeat(700), &[]).ends_with('…'));
    }

    #[test]
    fn what_names_a_path_stays_in_the_hub() {
        let step = Arc::new(SheetStep::new(None));
        // A session as `start` makes one, without a child.
        *step.lock() = Some(Session {
            id: 1,
            key: "alpha.nvim-0123456789ab".to_owned(),
            sheet: "smoke".to_owned(),
            root: PathBuf::from("/td/alpha.nvim"),
            stdin: None,
            kill: None,
            running: true,
            started: false,
            seq: 0,
            replay: Replay::default(),
            pictures: BTreeMap::new(),
            stderr: String::new(),
            astray: None,
        });
        let mut rx = step.subscribe();
        step.on_line(
            1,
            r#"{"schema":"testing-sheet-session/1","event":"start","id":"s","root":"/td/alpha.nvim","path":"/td/alpha.nvim/TESTS/sheets/smoke.sheet.lua","steps":[]}"#,
        );
        step.on_line(
            1,
            r#"{"schema":"testing-sheet-session/1","event":"step-after","size":"80x24","n":1,"status":"pass","ms":3,"image":"/td/cache/step-1.png","shot":""}"#,
        );
        let state = step.state();
        let text = state.to_string();
        assert!(!text.contains("/td/"), "{text}");
        assert_eq!(state["events"][0]["event"], "start");
        assert!(state["events"][0].get("root").is_none());
        assert_eq!(state["events"][1]["image"], true);
        assert!(state["events"][1].get("shot").is_none());
        assert_eq!(state["seq"], 2);
        assert_eq!(
            step.picture("80x24", 1, "image"),
            Some(PathBuf::from("/td/cache/step-1.png"))
        );
        assert_eq!(step.picture("80x24", 1, "shot"), None);
        assert_eq!(step.picture("80x24", 2, "image"), None);
        assert_eq!(step.picture("80x24", 1, "other"), None);
        // The stream carries the same, without the paths.
        let first = rx.try_recv().unwrap();
        assert!(first.starts_with("event: sheet-step\ndata: "), "{first}");
        assert!(!first.contains("/td/"), "{first}");
        let second = rx.try_recv().unwrap();
        assert!(second.contains(r#""image":true"#), "{second}");
    }

    #[test]
    fn the_replay_holds_the_sheet_the_sizes_every_step_and_the_last_stop() {
        let step = Arc::new(SheetStep::new(None));
        *step.lock() = Some(Session {
            id: 1,
            key: "k-0123456789ab".to_owned(),
            sheet: "s".to_owned(),
            root: PathBuf::from("/td"),
            stdin: None,
            kill: None,
            running: true,
            started: false,
            seq: 0,
            replay: Replay::default(),
            pictures: BTreeMap::new(),
            stderr: String::new(),
            astray: None,
        });
        let lines = [
            r#"{"schema":"testing-sheet-session/1","event":"start","id":"s","steps":[]}"#,
            r#"{"schema":"testing-sheet-session/1","event":"size","size":"80x24","state":"open"}"#,
            r#"{"schema":"testing-sheet-session/1","event":"paused","size":"80x24","n":1,"reason":"start"}"#,
            r#"{"schema":"testing-sheet-session/1","event":"step-before","size":"80x24","n":1}"#,
            r#"{"schema":"testing-sheet-session/1","event":"step-after","size":"80x24","n":1,"status":"pass","ms":1}"#,
            r#"{"schema":"testing-sheet-session/1","event":"error","message":"x"}"#,
            r#"{"schema":"testing-sheet-session/1","event":"note","size":"80x24","n":1,"text":"seen","time":"t"}"#,
            r#"{"schema":"testing-sheet-session/1","event":"ask","size":"80x24","n":2,"text":"ok?"}"#,
            "not json at all",
        ];
        for line in lines {
            step.on_line(1, line);
        }
        let events: Vec<String> = step.state()["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["event"].as_str().unwrap().to_owned())
            .collect();
        // A note stays for a page that comes later, in front of where the run stands.
        assert_eq!(events, ["start", "size", "step-after", "note", "ask"]);
        assert_eq!(step.state()["seq"], 8);
        // The answer of the question ends it; the end takes the stop away.
        step.on_line(
            1,
            r#"{"schema":"testing-sheet-session/1","event":"step-after","size":"80x24","n":2,"status":"pass","ms":1}"#,
        );
        step.on_line(
            1,
            r#"{"schema":"testing-sheet-session/1","event":"end","status":"pass","ok":true}"#,
        );
        let events: Vec<String> = step.state()["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["event"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            events,
            ["start", "size", "step-after", "step-after", "note", "end"]
        );
        // An exit after an `end` is no event; one without is the hub's note.
        step.on_exit(1, Some(0));
        assert!(step.state()["events"].as_array().unwrap().len() == 6);
        assert_eq!(step.state()["running"], false);
    }
}
