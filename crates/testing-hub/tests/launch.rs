//! A run through the engine from the app (`run/*`), against a stand-in for the engine's command line
//! (`tests/fixtures/fake-run.lua`, run by the real Neovim as the hub runs the engine: `TESTING_NVIM`, else the `nvim`
//! on the `PATH`; without one the tests say so and pass, unless `TESTING_HUB_REQUIRE_ENGINE=1`) and an engine in the
//! process that names the root of a repository for a key of the fleet. The stand-in writes the arguments it was given
//! into `<root>/argv.txt`, so what the hub asked of the engine is what is asserted.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use appkit_core::authn::{ManualClock, Secret, CODE_BYTES, TOKEN_BYTES};
use appkit_hub::engine::{CallFuture, Engine, Method, Reply};
use appkit_hub::{HubStatus, Limits, RunningServer};
use appkit_scratch::Scratch;
use serde_json::{json, Value};
use testing_hub::engine::Ask;
use testing_hub::sheetstep::StepProgram;
use testing_hub::Origins;

mod common;

use common::{get, post, session};

const FLEET: &str = include_str!("fixtures/golden/fleet.json");
const HELLO: &str = include_str!("fixtures/golden/hello.json");

/// The key of `alpha.nvim` in the golden fleet: the repository of the tasks of the area `alpha.nvim`.
const KEY: &str = "alpha.nvim-356bf237c7fc";
/// The task one sheet of the fixture names.
const TASK: &str = "alpha.nvim/try-it";

/// The root the engine names for every key, with its sheets.
struct Fixture {
    dir: Scratch,
}

impl Fixture {
    fn new() -> Self {
        let dir = Scratch::new("launch");
        let sheets = dir.join("root").join("TESTS").join("sheets");
        std::fs::create_dir_all(&sheets).unwrap();
        std::fs::write(
            sheets.join("try-it.sheet.lua"),
            format!("return {{\n  id = \"try-it\",\n  task = \"{TASK}\",\n}}\n"),
        )
        .unwrap();
        std::fs::write(
            sheets.join("other.sheet.lua"),
            "return {\n  task = \"alpha.nvim/other\",\n}\n",
        )
        .unwrap();
        std::fs::write(
            sheets.join("red.sheet.lua"),
            "return {\n  task = \"alpha.nvim/red\",\n}\n",
        )
        .unwrap();
        Self { dir }
    }

    fn root(&self) -> PathBuf {
        self.dir.join("root")
    }

    /// The state folder the hub reads.
    fn state(&self) -> PathBuf {
        self.dir.join("state")
    }

    /// The arguments the stand-in was given after its script, and its working folder.
    fn argv(&self) -> Vec<String> {
        std::fs::read_to_string(self.root().join("argv.txt"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

/// `hello` and `fleet` as the golden documents, `repo` in the local form with the root of the fixture.
struct RootEngine {
    root: PathBuf,
}

impl Engine for RootEngine {
    fn call<'a>(&'a self, method: Method, params: Option<&'a Value>) -> CallFuture<'a> {
        Box::pin(async move {
            let text = match method.as_str() {
                "hello" => HELLO.to_owned(),
                "fleet" => FLEET.to_owned(),
                "repo" => {
                    let params = params.cloned().unwrap_or(Value::Null);
                    assert_eq!(params["export"], false, "the local form: {params}");
                    json!({
                        "schema": "testing-export/1",
                        "kind": "repo",
                        "form": "local",
                        "repo": { "key": params["repo"], "root": self.root.display().to_string() },
                    })
                    .to_string()
                }
                other => panic!("the engine was asked {other}"),
            };
            Ok(Reply {
                kind: method.reply_kind().to_owned(),
                text: text.trim_end().to_owned(),
            })
        })
    }
}

fn nvim() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("TESTING_NVIM") {
        return Ok(PathBuf::from(path));
    }
    let name = if cfg!(windows) { "nvim.exe" } else { "nvim" };
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
        .ok_or_else(|| "no nvim on the PATH and no TESTING_NVIM".to_owned())
}

fn program_or_skip() -> Option<StepProgram> {
    match nvim() {
        Ok(nvim) => {
            let script = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("fake-run.lua");
            let env: Vec<(OsString, OsString)> =
                ["PATH", "SystemRoot", "TEMP", "TMP", "HOME", "USERPROFILE"]
                    .iter()
                    .filter_map(|n| std::env::var_os(n).map(|v| (OsString::from(n), v)))
                    .collect();
            Some(StepProgram {
                program: nvim,
                args: ["--headless", "-u", "NONE", "-i", "NONE", "-n", "-l"]
                    .iter()
                    .map(OsString::from)
                    .chain(std::iter::once(script.into_os_string()))
                    .collect(),
                env,
                cwd: None,
            })
        }
        Err(why) => {
            assert!(
                std::env::var_os("TESTING_HUB_REQUIRE_ENGINE").is_none_or(|v| v != "1"),
                "TESTING_HUB_REQUIRE_ENGINE=1 and no Neovim: {why}"
            );
            eprintln!("skipped: {why}");
            None
        }
    }
}

struct Hub {
    server: RunningServer,
    token: String,
}

async fn start_hub(fx: &Fixture, program: Option<StepProgram>) -> Hub {
    start_hub_with(fx, program, None).await
}

/// With the vault of the tasks the person named, as `tasks_vault` of the settings does.
async fn start_hub_with(
    fx: &Fixture,
    program: Option<StepProgram>,
    tasks_vault: Option<PathBuf>,
) -> Hub {
    let control = Secret::generate(TOKEN_BYTES).unwrap();
    let server = testing_hub::start(testing_hub::Start {
        addr: "127.0.0.1:0".parse().unwrap(),
        limits: Limits::default(),
        clock: Arc::new(ManualClock::new()),
        pairing_ttl: Duration::from_secs(60),
        control_secret: control.clone(),
        instance: Secret::generate(CODE_BYTES).unwrap(),
        status: HubStatus {
            engine_ready: true,
            problem: None,
            capabilities: vec!["hello".into(), "fleet".into(), "repo".into()],
        },
        engine: Some(Arc::new(RootEngine { root: fx.root() }) as Arc<dyn Engine>),
        ask: Ask {
            state_dir: Some(fx.state()),
            ..Ask::default()
        },
        origins: Origins::default(),
        watch: None,
        step: program,
        tasks_vault,
        ui: testing_hub::ui::UiFiles::none(),
        warm: false,
        phone: None,
        rerun: None,
    })
    .await
    .expect("the hub starts");
    let token = session(&server, &control).await;
    Hub { server, token }
}

fn route(name: &str) -> String {
    testing_hub::IDENTITY.route(name)
}

impl Hub {
    async fn post(&self, name: &str, body: &Value) -> appkit_testkit::wire::Reply {
        post(
            &self.server,
            &route(name),
            Some(&self.token),
            &body.to_string(),
        )
        .await
    }

    async fn get(&self, path: &str) -> appkit_testkit::wire::Reply {
        get(&self.server, path, Some(&self.token)).await
    }

    async fn state(&self) -> Value {
        let r = self.get(&route("run")).await;
        assert_eq!(r.status, 200, "{r:?}");
        r.json()
    }

    /// The state once the run has ended, within a patience.
    async fn ended(&self) -> Value {
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            let state = self.state().await;
            if state["run"]["running"] == false {
                return state;
            }
            assert!(Instant::now() < end, "the run did not end: {state}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[tokio::test]
async fn the_suite_of_a_repository_runs_through_the_engines_command_line() {
    let Some(program) = program_or_skip() else {
        return;
    };
    let fx = Fixture::new();
    let hub = start_hub(&fx, Some(program)).await;
    assert_eq!(hub.state().await["run"], Value::Null);
    let r = hub.post("run/start", &json!({ "key": KEY })).await;
    assert_eq!(r.status, 202, "{r:?}");
    assert_eq!(r.json()["run"]["what"], "suite");
    let state = hub.ended().await;
    assert_eq!(state["run"]["verdict"], "green", "{state}");
    assert_eq!(state["run"]["exit"], 0);
    // The root is the engine's, taken out of what the page sees.
    let output = state["run"]["output"].as_str().unwrap();
    assert!(output.contains("ran in <path>"), "{output}");
    assert!(output.contains("GREEN 3 passed"), "{output}");
    let argv = fx.argv();
    let root = fx.root().display().to_string();
    // The result goes into the state folder the hub reads.
    let state_dir = fx.state().display().to_string();
    assert_eq!(
        argv[..5],
        [
            "run",
            "--root",
            root.as_str(),
            "--state-dir",
            state_dir.as_str()
        ],
        "{argv:?}"
    );
    assert_eq!(
        argv.len(),
        6,
        "nothing but the command, the root, the state and the folder: {argv:?}"
    );
    // It runs in the root, as a run of the command line does.
    let cwd = argv[5].strip_prefix("cwd=").unwrap().to_lowercase();
    let want = fx
        .root()
        .display()
        .to_string()
        .replace('\\', "/")
        .to_lowercase();
    assert!(
        cwd.ends_with(want.trim_start_matches("//?/")),
        "{cwd} vs {want}"
    );
}

#[tokio::test]
async fn a_file_runs_red_and_a_second_run_waits_for_the_first() {
    let Some(program) = program_or_skip() else {
        return;
    };
    let fx = Fixture::new();
    let hub = start_hub(&fx, Some(program)).await;
    let r = hub
        .post("run/start", &json!({ "key": KEY, "file": "red" }))
        .await;
    assert_eq!(r.status, 202, "{r:?}");
    let state = hub.ended().await;
    assert_eq!(state["run"]["verdict"], "red", "{state}");
    assert_eq!(state["run"]["file"], "red");
    assert!(
        state["run"]["output"]
            .as_str()
            .unwrap()
            .contains("<path>/TESTS/red_spec.lua:3"),
        "{state}"
    );
    assert_eq!(fx.argv()[3..5], ["--file", "red"]);

    // One run at a time: while one runs, a second is refused and names it.
    let r = hub
        .post("run/start", &json!({ "key": KEY, "file": "slow" }))
        .await;
    assert_eq!(r.status, 202, "{r:?}");
    let end = Instant::now() + Duration::from_secs(20);
    while !fx.root().join("slow.started").is_file() {
        assert!(Instant::now() < end, "the slow run did not start");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let r = hub.post("run/start", &json!({ "key": KEY })).await;
    assert_eq!(r.status, 409, "{r:?}");
    assert_eq!(r.json()["error"]["code"], "busy");
    let r = hub.post("run/task", &json!({ "task": TASK })).await;
    assert_eq!(r.status, 409, "{r:?}");
    assert_eq!(hub.state().await["run"]["running"], true);
    // The hub stops: the run ends with it.
    hub.server.shutdown().await;
}

#[tokio::test]
async fn the_hub_knows_the_sheets_of_a_task_and_runs_only_those() {
    let Some(program) = program_or_skip() else {
        return;
    };
    let fx = Fixture::new();
    let hub = start_hub(&fx, Some(program)).await;
    // The sheets of the task, in the repository of its area.
    let r = hub
        .get(&format!("{}?task={TASK}", route("run/task-sheets")))
        .await;
    assert_eq!(r.status, 200, "{r:?}");
    assert_eq!(
        r.json(),
        json!({ "schema": 1, "kind": "hub.task-sheets", "task": TASK, "key": KEY, "sheets": ["try-it"] })
    );
    let r = hub
        .get(&format!(
            "{}?task=alpha.nvim/none",
            route("run/task-sheets")
        ))
        .await;
    assert_eq!(r.json()["sheets"], json!([]), "{r:?}");
    // A task no sheet names is not run: the engine is not even started.
    let r = hub
        .post("run/task", &json!({ "task": "alpha.nvim/none" }))
        .await;
    assert_eq!(r.status, 404, "{r:?}");
    assert_eq!(r.json()["error"]["code"], "no_sheet");
    assert!(fx.argv().is_empty(), "{:?}", fx.argv());
    // An area with no repository of the fleet, or a key not of the fleet.
    let r = hub.post("run/task", &json!({ "task": "nowhere/x" })).await;
    assert_eq!(r.json()["error"]["code"], "no_repo", "{r:?}");
    let r = hub
        .post(
            "run/task",
            &json!({ "task": TASK, "key": "zeta.nvim-0123456789ab" }),
        )
        .await;
    assert_eq!(r.json()["error"]["code"], "no_repo", "{r:?}");
    // The task's sheets run, through `testing sheets run --task`.
    let r = hub.post("run/task", &json!({ "task": TASK })).await;
    assert_eq!(r.status, 202, "{r:?}");
    let state = hub.ended().await;
    assert_eq!(state["run"]["what"], "task");
    assert_eq!(state["run"]["task"], TASK);
    assert_eq!(state["run"]["verdict"], "green", "{state}");
    let root = fx.root().display().to_string();
    assert_eq!(
        fx.argv()[..7],
        [
            "sheets",
            "run",
            "--task",
            TASK,
            "--root",
            root.as_str(),
            "--result"
        ]
    );
    // What the engine said of the sheets is read from its document of the run.
    assert_eq!(state["run"]["sheets_red"], false, "{state}");
    // No vault was named for this hub: the run writes into no task, and the state says nothing of a block.
    assert!(
        !fx.argv().contains(&"--tasks".to_owned()),
        "{:?}",
        fx.argv()
    );
    assert_eq!(state["run"]["block"], Value::Null);
    // The engine's refusal is the verdict `refused` (here: a sheet of another task, which the stand-in refuses).
    let r = hub
        .post("run/task", &json!({ "task": "alpha.nvim/other" }))
        .await;
    assert_eq!(r.status, 202, "{r:?}");
    let state = hub.ended().await;
    assert_eq!(state["run"]["verdict"], "refused", "{state}");
    assert_eq!(state["run"]["exit"], 2);
}

#[tokio::test]
async fn with_a_vault_the_sheets_of_a_task_write_into_it_and_no_other_run_does() {
    let Some(program) = program_or_skip() else {
        return;
    };
    let fx = Fixture::new();
    let vault = fx.dir.join("tasks-vault");
    std::fs::create_dir_all(&vault).unwrap();
    let hub = start_hub_with(&fx, Some(program), Some(vault.clone())).await;
    let root = fx.root().display().to_string();
    let named = vault.display().to_string();
    // The vault is the hub's own: a request cannot name one.
    for body in [
        json!({ "task": TASK, "tasks": "/elsewhere" }),
        json!({ "task": TASK, "vault": "/elsewhere" }),
    ] {
        let r = hub.post("run/task", &body).await;
        assert_eq!(r.status, 400, "{body}: {r:?}");
    }
    let r = hub.post("run/task", &json!({ "task": TASK })).await;
    assert_eq!(r.status, 202, "{r:?}");
    let state = hub.ended().await;
    assert_eq!(state["run"]["verdict"], "green", "{state}");
    assert_eq!(
        fx.argv()[..9],
        [
            "sheets",
            "run",
            "--task",
            TASK,
            "--root",
            root.as_str(),
            "--result",
            "--tasks",
            named.as_str()
        ]
    );
    assert_eq!(
        state["run"]["block"],
        json!({ "written": true, "outcome": "changed" })
    );
    // Green sheets and a task that could not be written: the engine's exit 1, told apart from a red sheet by its
    // own word on the block. The vault is taken out of what the page sees.
    let r = hub
        .post("run/task", &json!({ "task": "alpha.nvim/other" }))
        .await;
    assert_eq!(r.status, 202, "{r:?}");
    let state = hub.ended().await;
    assert_eq!(state["run"]["exit"], 1, "{state}");
    assert_eq!(state["run"]["verdict"], "not_written");
    assert_eq!(state["run"]["sheets_red"], false);
    assert_eq!(
        state["run"]["block"],
        json!({ "written": false, "outcome": "failed" })
    );
    let output = state["run"]["output"].as_str().unwrap();
    assert!(output.contains("conflict: <path>/x.md changed"), "{output}");
    assert!(!state.to_string().contains("tasks-vault"), "{state}");
    // The engine's document is the hub's to read, not a line for the person.
    assert!(!output.contains("testing-sheet-run"), "{output}");
    // The other exit 1: a sheet is red and the task says so. That is the verdict red.
    let r = hub
        .post("run/task", &json!({ "task": "alpha.nvim/red" }))
        .await;
    assert_eq!(r.status, 202, "{r:?}");
    let state = hub.ended().await;
    assert_eq!(state["run"]["exit"], 1, "{state}");
    assert_eq!(state["run"]["verdict"], "red");
    assert_eq!(state["run"]["sheets_red"], true);
    assert_eq!(
        state["run"]["block"],
        json!({ "written": true, "outcome": "changed" })
    );
    // A run of the suite or of a file never gets the vault.
    let r = hub.post("run/start", &json!({ "key": KEY })).await;
    assert_eq!(r.status, 202, "{r:?}");
    hub.ended().await;
    assert!(
        !fx.argv().contains(&"--tasks".to_owned()),
        "{:?}",
        fx.argv()
    );
}

/// Wait until `file` is there and has something in it.
async fn appears(file: &Path) {
    let end = Instant::now() + Duration::from_secs(30);
    while std::fs::metadata(file).map_or(0, |m| m.len()) == 0 {
        assert!(Instant::now() < end, "{} did not appear", file.display());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Wait until `file` has not grown for a whole second: the process that writes it is gone. It writes every tenth of
/// a second for a minute, so a file that still grows after 30 s is a process that was left running.
async fn goes_quiet(file: &Path, what: &str) {
    let size = || std::fs::metadata(file).map_or(0, |m| m.len());
    let end = Instant::now() + Duration::from_secs(30);
    let (mut last, mut since) = (size(), Instant::now());
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let now = size();
        if now != last {
            (last, since) = (now, Instant::now());
        } else if since.elapsed() >= Duration::from_secs(1) {
            return;
        }
        assert!(Instant::now() < end, "{what}");
    }
}

#[tokio::test]
async fn an_abort_ends_the_run_with_every_process_it_started_and_so_does_a_stop_of_the_hub() {
    let Some(program) = program_or_skip() else {
        return;
    };
    let fx = Fixture::new();
    let beat = fx.root().join("beat.txt");
    let hub = start_hub(&fx, Some(program)).await;
    // Nothing runs: there is nothing to abort.
    let r = hub.post("run/abort", &json!({})).await;
    assert_eq!(r.status, 409, "{r:?}");
    assert_eq!(r.json()["error"]["code"], "not_running");
    let tree = json!({ "key": KEY, "file": "tree" });
    let r = hub.post("run/start", &tree).await;
    assert_eq!(r.status, 202, "{r:?}");
    appears(&beat).await;
    // An abort names nothing: there is one run, and no word of a request reaches a process.
    for body in [
        json!({ "key": KEY }),
        json!({ "signal": "TERM" }),
        json!([]),
    ] {
        let r = hub.post("run/abort", &body).await;
        assert_eq!(r.status, 400, "{body}: {r:?}");
    }
    assert_eq!(hub.state().await["run"]["running"], true);
    let r = hub.post("run/abort", &json!({})).await;
    assert_eq!(r.status, 202, "{r:?}");
    assert_eq!(r.json()["kind"], "hub.run", "{r:?}");
    let state = hub.ended().await;
    assert_eq!(state["run"]["verdict"], "stopped", "{state}");
    assert_eq!(state["run"]["exit"], Value::Null);
    // Not only the engine: the process it started is gone too.
    goes_quiet(&beat, "a process of the run outlived its abort").await;
    // The run is over: a second abort finds none, and the next run may start.
    let r = hub.post("run/abort", &json!({})).await;
    assert_eq!(r.status, 409, "{r:?}");

    // The same when the hub stops.
    std::fs::remove_file(&beat).unwrap();
    let r = hub.post("run/start", &tree).await;
    assert_eq!(r.status, 202, "{r:?}");
    appears(&beat).await;
    hub.server.shutdown().await;
    goes_quiet(&beat, "a process of the run outlived the hub").await;
}

#[tokio::test]
async fn a_request_names_a_key_a_file_or_a_task_and_nothing_else() {
    let fx = Fixture::new();
    // No program: whatever passes the checks would answer 503, so a 400 is the check.
    let hub = start_hub(&fx, None).await;
    for (name, body) in [
        ("run/start", json!({})),
        ("run/start", json!({ "key": "../x" })),
        ("run/start", json!({ "key": KEY, "file": "--trust" })),
        ("run/start", json!({ "key": KEY, "file": "../../x" })),
        ("run/start", json!({ "key": KEY, "file": 3 })),
        ("run/start", json!({ "key": KEY, "root": "/tmp" })),
        ("run/start", json!({ "key": KEY, "args": ["ci", "trust"] })),
        ("run/task", json!({})),
        ("run/task", json!({ "task": "no-slash" })),
        ("run/task", json!({ "task": "a/b/c" })),
        ("run/task", json!({ "task": "-a/b" })),
        ("run/task", json!({ "task": TASK, "key": "x" })),
        ("run/task", json!({ "task": TASK, "sheet": "try-it" })),
    ] {
        let r = hub.post(name, &body).await;
        assert_eq!(r.status, 400, "{name} {body}: {r:?}");
    }
    for query in ["", "?task=x", "?task=a/b&key=x", "?task=a/b&other=1"] {
        let r = hub
            .get(&format!("{}{query}", route("run/task-sheets")))
            .await;
        assert_eq!(r.status, 400, "{query}: {r:?}");
    }
    let r = hub.post("run/start", &json!({ "key": KEY })).await;
    assert_eq!(r.status, 503, "{r:?}");
    assert_eq!(r.json()["error"]["code"], "run_unavailable");
    let r = hub.get(&format!("{}?x=1", route("run"))).await;
    assert_eq!(r.status, 400, "{r:?}");
    assert_eq!(hub.state().await["available"], false);
}

#[tokio::test]
async fn a_process_the_engine_leaves_behind_does_not_hold_the_run() {
    let Some(program) = program_or_skip() else {
        return;
    };
    let fx = Fixture::new();
    let hub = start_hub(&fx, Some(program)).await;
    let r = hub
        .post("run/start", &json!({ "key": KEY, "file": "orphan" }))
        .await;
    assert_eq!(r.status, 202, "{r:?}");
    // The process it left keeps the pipes for 25 s; the run is over long before.
    let start = Instant::now();
    let end = start + Duration::from_secs(15);
    let state = loop {
        let state = hub.state().await;
        if state["run"]["running"] == false {
            break state;
        }
        assert!(
            Instant::now() < end,
            "the run is held by what it left: {state}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(state["run"]["verdict"], "green", "{state}");
    // And the next run may start.
    let r = hub.post("run/start", &json!({ "key": KEY })).await;
    assert_eq!(r.status, 202, "{r:?}");
    hub.ended().await;
}
