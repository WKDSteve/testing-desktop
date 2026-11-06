//! A stepped run of a sheet from the page (`sheet-step`), against a stand-in for `testing sheets step`
//! (`tests/fixtures/fake-step.lua`, run by the real Neovim as the hub runs the engine: `TESTING_NVIM`, else the
//! `nvim` on the `PATH`; without one the tests say so and pass, unless `TESTING_HUB_REQUIRE_ENGINE=1`) and an engine
//! in the process that names the root of a repository for its key. Every command the hub writes reaches the child
//! as a line in `<root>/commands.txt`, so what the child saw is what is asserted.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use appkit_core::authn::{ManualClock, Secret, CODE_BYTES, TOKEN_BYTES};
use appkit_hub::engine::{CallFuture, Engine, EngineError, Method, Reply};
use appkit_hub::{HubStatus, Limits, RunningServer};
use appkit_scratch::Scratch;
use serde_json::{json, Value};
use testing_hub::engine::Ask;
use testing_hub::sheetstep::{StepProgram, GRACE};
use testing_hub::Origins;

mod common;

use common::{get, post, session, Stream};

const FLEET: &str = include_str!("fixtures/golden/fleet.json");
const HELLO: &str = include_str!("fixtures/golden/hello.json");

/// The key of the one repository the engine of these tests knows.
const KEY: &str = "alpha.nvim-0123456789ab";

/// The root the engine names for `KEY`, and what the child leaves there.
struct Fixture {
    dir: Scratch,
}

impl Fixture {
    fn new() -> Self {
        let dir = Scratch::new("sheetstep");
        let sheets = dir.join("root").join("TESTS").join("sheets");
        std::fs::create_dir_all(&sheets).unwrap();
        for name in ["green", "ask", "hang"] {
            std::fs::write(sheets.join(format!("{name}.sheet.lua")), "return {}\n").unwrap();
        }
        std::fs::write(sheets.join("README.md"), "not a sheet\n").unwrap();
        // A file of the root the child never names: the hub must not serve it.
        std::fs::write(dir.join("root").join("secret.png"), b"\x89PNG not reported").unwrap();
        Self { dir }
    }

    fn root(&self) -> PathBuf {
        self.dir.join("root")
    }

    /// The lines the child read on its stdin, in order.
    fn commands(&self) -> Vec<String> {
        std::fs::read_to_string(self.root().join("commands.txt"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn left(&self, name: &str) -> PathBuf {
        self.root().join(name)
    }
}

/// An engine in the process: `hello` and `fleet` as the golden documents, `repo` in the local form with the root
/// of the fixture for `KEY` and exit 2 for any other key.
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
                    assert_eq!(params["export"], false, "the local form is asked: {params}");
                    assert!(
                        params.get("areas").is_none() && params.get("github_token").is_none(),
                        "no data classes, no token: {params}"
                    );
                    if params["repo"] != KEY {
                        return Err(EngineError::Failed {
                            code: Some(2),
                            detail: "testing call: no repository below $TESTING_REPOS_ROOT"
                                .to_owned(),
                        });
                    }
                    json!({
                        "schema": "testing-export/1",
                        "kind": "repo",
                        "form": "local",
                        "repo": { "key": KEY, "name": "alpha.nvim", "root": self.root.display().to_string() },
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

/// The Neovim that runs the stand-in, or why there is none.
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

/// The program of the stand-in, or `None` after saying why (a failure in CI).
fn program_or_skip() -> Option<StepProgram> {
    match nvim() {
        Ok(nvim) => {
            let script = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("fake-step.lua");
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
        ask: Ask::default(),
        origins: Origins::default(),
        watch: None,
        step: program,
        ui: testing_hub::ui::UiFiles::none(),
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
    async fn get(&self, path: &str) -> appkit_testkit::wire::Reply {
        get(&self.server, path, Some(&self.token)).await
    }

    async fn post(&self, name: &str, body: &str) -> appkit_testkit::wire::Reply {
        post(&self.server, &route(name), Some(&self.token), body).await
    }

    async fn start(&self, sheet: &str) -> appkit_testkit::wire::Reply {
        self.post(
            "sheet-step/start",
            &json!({ "key": KEY, "sheet": sheet }).to_string(),
        )
        .await
    }

    async fn command(&self, body: Value) -> appkit_testkit::wire::Reply {
        self.post("sheet-step/command", &body.to_string()).await
    }

    async fn state(&self) -> Value {
        let r = self.get(&route("sheet-step")).await;
        assert_eq!(r.status, 200, "{r:?}");
        r.json()
    }

    /// The state once `running` is false, within a patience.
    async fn ended(&self) -> Value {
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            let state = self.state().await;
            if state["running"] == false {
                return state;
            }
            assert!(Instant::now() < end, "the run did not end: {state}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// The names of the events of a state, in order.
fn events(state: &Value) -> Vec<String> {
    state["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["event"].as_str().unwrap().to_owned())
        .collect()
}

/// Wait until a file is there.
async fn appears(path: &Path) {
    let end = Instant::now() + Duration::from_secs(15);
    while !path.exists() {
        assert!(Instant::now() < end, "{} did not appear", path.display());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn the_sheets_of_a_repository_are_listed_by_name_and_nothing_else() {
    let fx = Fixture::new();
    let Some(program) = program_or_skip() else {
        return;
    };
    let hub = start_hub(&fx, Some(program)).await;
    let r = hub
        .get(&format!("{}?key={KEY}", route("sheet-step/sheets")))
        .await;
    assert_eq!(r.status, 200, "{r:?}");
    assert_eq!(r.json()["sheets"], json!(["ask", "green", "hang"]));
    assert_eq!(r.json()["key"], KEY);
    assert!(
        !r.body
            .contains(&fx.root().display().to_string().replace('\\', "/"))
            && !r.body.contains(&fx.root().display().to_string()),
        "a path in the answer: {}",
        r.body
    );
    // Another key: the engine knows none. A query that is no key never reaches it.
    let gone = hub
        .get(&format!(
            "{}?key=gone.nvim-0123456789ab",
            route("sheet-step/sheets")
        ))
        .await;
    assert_eq!(gone.status, 404, "{gone:?}");
    for bad in ["", "?key=..", "?key=C:/td", "?root=x"] {
        let r = hub
            .get(&format!("{}{bad}", route("sheet-step/sheets")))
            .await;
        assert_eq!(r.status, 400, "{bad}: {r:?}");
    }
}

#[tokio::test]
async fn a_run_goes_step_by_step_and_its_lines_come_on_the_stream_without_a_path() {
    let fx = Fixture::new();
    let Some(program) = program_or_skip() else {
        return;
    };
    let hub = start_hub(&fx, Some(program)).await;
    let before = hub.state().await;
    assert_eq!(before["running"], false);
    assert_eq!(before["available"], true);
    assert!(before["session"].is_null());

    let mut stream = Stream::open(&hub.server, &route("events"), &hub.token).await;
    stream.until("event: hello").await;

    let started = hub.start("green").await;
    assert_eq!(started.status, 200, "{started:?}");
    let state = started.json();
    assert_eq!(state["running"], true);
    assert_eq!(state["sheet"], "green");
    assert_eq!(state["key"], KEY);
    // The answer comes with the first event; what follows comes on the stream.
    assert_eq!(events(&state)[0], "start");
    assert_eq!(
        state["events"][0]["steps"].as_array().map(Vec::len),
        Some(2)
    );
    // Nothing of the root reaches the page: `root` and `path` of `start` are taken out.
    let root_text = fx.root().display().to_string();
    for shown in [root_text.clone(), root_text.replace('\\', "/")] {
        assert!(!started.body.contains(&shown), "a path: {}", started.body);
    }
    assert!(state["events"][0].get("root").is_none());
    assert!(state["events"][0].get("path").is_none());
    stream.until(r#""event":"paused""#).await;

    // One step: its picture is named as a yes, never as a file.
    let r = hub.command(json!({ "command": "next" })).await;
    assert_eq!(r.status, 200, "{r:?}");
    stream
        .until(r#""event":"step-after","image":true,"ms":5,"n":1"#)
        .await;
    assert!(!stream.seen.contains("evidence"), "{}", stream.seen);
    assert!(stream.seen.contains(r#""image":true"#), "{}", stream.seen);

    // A breakpoint, cleared again, and an `until`: the lines the child reads are built by the hub.
    assert_eq!(
        hub.command(json!({ "command": "break", "n": 2 }))
            .await
            .status,
        200
    );
    assert_eq!(
        hub.command(json!({ "command": "clear", "n": 2 }))
            .await
            .status,
        200
    );
    assert_eq!(
        hub.command(json!({ "command": "until", "n": 2 }))
            .await
            .status,
        200
    );
    let state = hub.ended().await;
    assert_eq!(
        events(&state),
        ["start", "size", "step-after", "step-after", "end"]
    );
    assert_eq!(state["events"][4]["status"], "pass");
    assert_eq!(state["events"][3]["screen"], json!(["~", "float", "~"]));
    assert_eq!(fx.commands(), ["next", "break 2", "clear 2", "until 2"]);
    // The stream carried every line, numbered, of this run.
    stream.until(r#""event":"end""#).await;
    assert!(
        stream.seen.contains(r#""seq":1,"session":1}"#),
        "{}",
        stream.seen
    );

    // The picture of step 1 is served by its step; a step without one, and a request that names a file, are not.
    let image = hub
        .get(&format!(
            "{}?size=80x24&n=1&kind=image",
            route("sheet-step/image")
        ))
        .await;
    assert_eq!(image.status, 200, "{image:?}");
    assert_eq!(image.header("content-type"), Some("image/png"));
    assert!(
        image.body.starts_with("\u{fffd}PNG") || image.body.starts_with("\u{89}PNG"),
        "{:?}",
        &image.body[..8]
    );
    for (query, status) in [
        ("size=80x24&n=2&kind=image", 404),
        ("size=80x24&n=1&kind=shot", 404),
        ("size=80x24&n=3&kind=image", 404),
        ("size=80x24&n=1&kind=secret", 400),
        ("size=80x24&n=1", 400),
        ("path=secret.png", 400),
        ("size=80x24&n=1&kind=image&path=secret.png", 400),
        ("size=../&n=1&kind=image", 400),
        ("", 400),
    ] {
        let r = hub
            .get(&format!("{}?{query}", route("sheet-step/image")))
            .await;
        assert_eq!(r.status, status, "{query}: {r:?}");
    }

    // A command after the end is refused; the state of the run that ended stays for the page.
    let late = hub.command(json!({ "command": "next" })).await;
    assert_eq!(late.status, 409, "{late:?}");
    assert_eq!(hub.state().await["sheet"], "green");
}

#[tokio::test]
async fn a_second_run_is_refused_while_one_runs_and_abort_closes_the_childs_stdin() {
    let fx = Fixture::new();
    let Some(program) = program_or_skip() else {
        return;
    };
    let hub = start_hub(&fx, Some(program)).await;
    let r = hub.start("hang").await;
    assert_eq!(r.status, 200, "{r:?}");
    appears(&fx.left("started")).await;
    let again = hub.start("green").await;
    assert_eq!(again.status, 409, "{again:?}");
    assert_eq!(again.json()["error"]["code"], "busy");
    assert!(again.json()["error"]["message"]
        .as_str()
        .unwrap()
        .contains("hang"));
    // Abort: the child is told, its stdin closes, it sees its end and leaves nothing behind.
    let r = hub.command(json!({ "command": "abort" })).await;
    assert_eq!(r.status, 200, "{r:?}");
    appears(&fx.left("ended")).await;
    let state = hub.ended().await;
    assert_eq!(
        state["events"].as_array().unwrap().last().unwrap()["status"],
        "aborted"
    );
    assert_eq!(fx.commands(), ["abort"]);
    // And a run can start again.
    let next = hub.start("green").await;
    assert_eq!(next.status, 200, "{next:?}");
    assert_eq!(next.json()["session"], 2);
    hub.command(json!({ "command": "abort" })).await;
    hub.ended().await;
}

#[tokio::test]
async fn a_refusal_of_the_engine_is_the_answer_of_start_without_a_path() {
    let fx = Fixture::new();
    let Some(program) = program_or_skip() else {
        return;
    };
    let hub = start_hub(&fx, Some(program)).await;
    let r = hub.start("refused").await;
    assert_eq!(r.status, 422, "{r:?}");
    let error = r.json()["error"].clone();
    assert_eq!(error["code"], "refused");
    assert_eq!(error["exit"], 2);
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("refused on the workstation"), "{message}");
    assert!(message.contains("<path>"), "{message}");
    assert!(!message.contains("sheetstep"), "{message}");
    // Nothing is left of it: no run, and the next start is free.
    assert!(hub.state().await["session"].is_null());
    let r = hub.start("green").await;
    assert_eq!(r.status, 200, "{r:?}");
    hub.command(json!({ "command": "abort" })).await;
    hub.ended().await;
}

#[tokio::test]
async fn a_question_waits_for_its_answer_and_the_text_goes_as_one_line() {
    let fx = Fixture::new();
    let Some(program) = program_or_skip() else {
        return;
    };
    let hub = start_hub(&fx, Some(program)).await;
    let r = hub.start("ask").await;
    assert_eq!(r.status, 200, "{r:?}");
    // The state names the question, for a page that comes now.
    let end = Instant::now() + Duration::from_secs(15);
    let state = loop {
        let state = hub.state().await;
        if events(&state).contains(&"ask".to_owned()) {
            break state;
        }
        assert!(Instant::now() < end, "{state}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(
        state["events"].as_array().unwrap().last().unwrap()["text"],
        "Does it sound right?"
    );
    let r = hub
        .command(json!({ "command": "answer", "yes": false, "text": "  the sound stutters  " }))
        .await;
    assert_eq!(r.status, 200, "{r:?}");
    // The failed step pauses; `next` ends it.
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        let state = hub.state().await;
        if events(&state).contains(&"paused".to_owned()) {
            break;
        }
        assert!(Instant::now() < end, "{state}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    hub.command(json!({ "command": "next" })).await;
    let state = hub.ended().await;
    assert_eq!(events(&state), ["start", "size", "step-after", "end"]);
    assert_eq!(state["events"][2]["status"], "fail");
    assert_eq!(state["events"][2]["reason"], "the sound stutters");
    assert_eq!(state["events"][3]["status"], "fail");
    assert_eq!(fx.commands(), ["answer no the sound stutters", "next"]);
}

#[tokio::test]
async fn what_is_not_a_command_of_the_list_never_reaches_the_child() {
    let fx = Fixture::new();
    let Some(program) = program_or_skip() else {
        return;
    };
    let hub = start_hub(&fx, Some(program)).await;
    // No run: a command is refused, whatever it is.
    let r = hub.command(json!({ "command": "next" })).await;
    assert_eq!(r.status, 409, "{r:?}");
    let r = hub.start("green").await;
    assert_eq!(r.status, 200, "{r:?}");
    for bad in [
        json!({ "command": "retry" }),
        json!({ "command": "next; abort" }),
        json!({ "command": "answer", "yes": true, "text": "a\nabort" }),
        json!({ "command": "until", "n": "2" }),
        json!({ "command": "next", "n": 1 }),
        json!(["next"]),
        json!("next"),
    ] {
        let r = hub.command(bad.clone()).await;
        assert_eq!(r.status, 400, "{bad}: {r:?}");
    }
    let r = hub.post("sheet-step/command", "not json").await;
    assert_eq!(r.status, 400, "{r:?}");
    let r = hub
        .post(
            "sheet-step/start",
            &json!({ "key": KEY, "sheet": "../x" }).to_string(),
        )
        .await;
    assert_eq!(r.status, 400, "{r:?}");
    let r = hub
        .post(
            "sheet-step/start",
            &json!({ "key": "C:/td", "sheet": "green" }).to_string(),
        )
        .await;
    assert_eq!(r.status, 400, "{r:?}");
    let r = hub
        .post(
            "sheet-step/start",
            &json!({ "key": KEY, "sheet": "green", "root": "C:/" }).to_string(),
        )
        .await;
    assert_eq!(r.status, 400, "{r:?}");
    hub.command(json!({ "command": "abort" })).await;
    hub.ended().await;
    assert_eq!(fx.commands(), ["abort"]);
}

/// The time of the last life sign of the child, and whether it stops.
async fn heartbeat_stops(alive: &Path) {
    appears(alive).await;
    // Past the grace the child is killed: its file stops changing.
    tokio::time::sleep(GRACE + Duration::from_millis(1500)).await;
    let before = std::fs::read_to_string(alive).unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    let after = std::fs::read_to_string(alive).unwrap();
    assert_eq!(before, after, "the child still writes");
}

#[tokio::test]
async fn a_child_that_ignores_its_stdin_is_killed_on_abort_and_when_the_hub_stops() {
    let fx = Fixture::new();
    let Some(program) = program_or_skip() else {
        return;
    };
    let hub = start_hub(&fx, Some(program)).await;
    let r = hub.start("deaf").await;
    assert_eq!(r.status, 200, "{r:?}");
    hub.command(json!({ "command": "abort" })).await;
    heartbeat_stops(&fx.left("alive")).await;
    let state = hub.ended().await;
    // No `end` of the child: the hub's own note says it went.
    assert_eq!(events(&state).last().map(String::as_str), Some("exit"));

    std::fs::remove_file(fx.left("alive")).unwrap();
    let r = hub.start("deaf").await;
    assert_eq!(r.status, 200, "{r:?}");
    appears(&fx.left("alive")).await;
    hub.server.shutdown().await;
    tokio::time::sleep(GRACE + Duration::from_millis(1500)).await;
    let before = std::fs::read_to_string(fx.left("alive")).unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    let after = std::fs::read_to_string(fx.left("alive")).unwrap();
    assert_eq!(before, after, "the child outlived the hub");
}

#[tokio::test]
async fn a_child_that_writes_a_line_beyond_the_limit_is_ended() {
    let fx = Fixture::new();
    let Some(program) = program_or_skip() else {
        return;
    };
    let hub = start_hub(&fx, Some(program)).await;
    let r = hub.start("chatty").await;
    assert_eq!(r.status, 200, "{r:?}");
    let state = hub.ended().await;
    assert_eq!(events(&state).last().map(String::as_str), Some("exit"));
}

#[tokio::test]
async fn the_post_routes_need_a_session_and_a_hub_without_an_engine_says_so() {
    let fx = Fixture::new();
    let hub = start_hub(&fx, None).await;
    for name in ["sheet-step/start", "sheet-step/command"] {
        let r = post(&hub.server, &route(name), None, "{}").await;
        assert_eq!(r.status, 401, "{name}: {r:?}");
    }
    // With a session but no program: the state says so, a start is refused.
    assert_eq!(hub.state().await["available"], false);
    let r = hub.start("green").await;
    assert_eq!(r.status, 503, "{r:?}");
    assert_eq!(r.json()["error"]["code"], "step_unavailable");
    let r = hub
        .get(&format!("{}?key={KEY}", route("sheet-step/sheets")))
        .await;
    assert_eq!(r.status, 503, "{r:?}");
}
