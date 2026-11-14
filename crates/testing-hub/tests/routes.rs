//! The read routes of the hub against an engine in the process, without Neovim: a fake that answers
//! with the golden documents of testing.nvim (`tests/fixtures/golden`, copies of
//! `TESTS/golden/testing-export-1/`, `-hello/` and `-findings/` at the commit of `engine.lock`) and
//! keeps what it was asked.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use appkit_core::authn::{ManualClock, Secret, CODE_BYTES, TOKEN_BYTES};
use appkit_hub::engine::{CallFuture, Engine, EngineError, Method, Reply};
use appkit_hub::{HubStatus, Limits, RunningServer};
use appkit_scratch::Scratch;
use serde_json::{json, Value};
use testing_hub::engine::Ask;
use testing_hub::github::{CiState, GithubToken};
use testing_hub::settings::Origin;
use testing_hub::store::RefreshOptions;
use testing_hub::{Origins, Watch};

mod common;

use common::{get, get_with, session, Stream};

const HELLO: &str = include_str!("fixtures/golden/hello.json");
const FLEET: &str = include_str!("fixtures/golden/fleet.json");
const REPO: &str = include_str!("fixtures/golden/repo.json");
const FINDINGS: &str = include_str!("fixtures/golden/findings.json");

/// The digests of the golden documents: their ETags.
const FLEET_DIGEST: &str = "sha256:e38c07f939e30153";
const REPO_DIGEST: &str = "sha256:961b3b906a1fd972";
/// The digest of the fleet after a change.
const NEW_DIGEST: &str = "sha256:7777777777777777";
/// The key of the repository of the golden `repo` document.
const BETA: &str = "beta.nvim-ac6d3a4ece7f";

/// How the fake answers `repo`.
#[derive(Clone, Copy)]
enum RepoAnswer {
    Golden,
    /// testing.nvim's "no such repository" or "not released": exit 2, no document.
    Exit2,
    Timeout,
}

/// An engine in the process. It answers with the golden documents and keeps every question.
struct FakeEngine {
    repo: Mutex<RepoAnswer>,
    fleet: Mutex<String>,
    asked: Mutex<Vec<(&'static str, Option<Value>)>>,
}

impl FakeEngine {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            repo: Mutex::new(RepoAnswer::Golden),
            fleet: Mutex::new(FLEET.to_owned()),
            asked: Mutex::new(Vec::new()),
        })
    }

    /// From now on the engine says the fleet has changed.
    fn change_the_fleet(&self) {
        let mut doc: Value = serde_json::from_str(FLEET).unwrap();
        doc["digest"] = json!(NEW_DIGEST);
        doc["rev"] = json!("rev:777777");
        *self.fleet.lock().unwrap() = doc.to_string();
    }

    fn asked(&self, method: &str) -> Vec<Option<Value>> {
        self.asked
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _)| *m == method)
            .map(|(_, p)| p.clone())
            .collect()
    }
}

impl Engine for FakeEngine {
    fn call<'a>(&'a self, method: Method, params: Option<&'a Value>) -> CallFuture<'a> {
        Box::pin(async move {
            self.asked
                .lock()
                .unwrap()
                .push((method.as_str(), params.cloned()));
            let text = match method.as_str() {
                "hello" => HELLO.to_owned(),
                "fleet" => self.fleet.lock().unwrap().clone(),
                "finding" => FINDINGS.to_owned(),
                "repo" => match *self.repo.lock().unwrap() {
                    RepoAnswer::Golden => REPO.to_owned(),
                    RepoAnswer::Exit2 => {
                        return Err(EngineError::Failed {
                            code: Some(2),
                            detail: "testing call: no repository below $TESTING_REPOS_ROOT"
                                .to_owned(),
                        });
                    }
                    RepoAnswer::Timeout => return Err(EngineError::Timeout),
                },
                other => panic!("the fake engine was asked {other}"),
            };
            Ok(Reply {
                kind: method.reply_kind().to_owned(),
                text: text.trim_end().to_owned(),
            })
        })
    }
}

struct Hub {
    server: RunningServer,
    token: String,
}

fn ready() -> HubStatus {
    HubStatus {
        engine_ready: true,
        problem: None,
        capabilities: vec!["hello".into(), "fleet".into(), "repo".into()],
    }
}

/// A whole path of no folder that exists: the hub only passes it on.
fn place(name: &str) -> PathBuf {
    PathBuf::from(if cfg!(windows) { "C:/td" } else { "/td" }).join(name)
}

fn ask() -> Ask {
    Ask {
        state_dir: Some(place("state")),
        areas: Some(place("areas.json")),
        ..Ask::default()
    }
}

async fn start(engine: Option<Arc<FakeEngine>>) -> Hub {
    start_with(engine, Limits::default(), None).await
}

/// A hub whose snapshot only a change renews, watching `state`.
async fn start_watching(engine: &Arc<FakeEngine>, state: &Path) -> Hub {
    start_with(
        Some(Arc::clone(engine)),
        Limits {
            snapshot_max_age: None,
            ..Limits::default()
        },
        Some(Watch {
            state_dir: state.to_path_buf(),
            refresh: RefreshOptions {
                debounce: Duration::from_millis(50),
                max_wait: Duration::from_millis(300),
                interval: Duration::from_secs(600),
            },
        }),
    )
    .await
}

async fn start_with(engine: Option<Arc<FakeEngine>>, limits: Limits, watch: Option<Watch>) -> Hub {
    start_asking(engine, limits, watch, ask(), CiState::NoToken).await
}

async fn start_asking(
    engine: Option<Arc<FakeEngine>>,
    limits: Limits,
    watch: Option<Watch>,
    ask: Ask,
    ci: CiState,
) -> Hub {
    start_warming(engine, limits, watch, ask, ci, false).await
}

/// [`start_asking`], and with `warm` the hub reads the fleet at once, as the program does.
async fn start_warming(
    engine: Option<Arc<FakeEngine>>,
    limits: Limits,
    watch: Option<Watch>,
    ask: Ask,
    ci: CiState,
    warm: bool,
) -> Hub {
    let control = Secret::generate(TOKEN_BYTES).unwrap();
    let has_engine = engine.is_some();
    let server = testing_hub::start(testing_hub::Start {
        addr: "127.0.0.1:0".parse().unwrap(),
        limits,
        clock: Arc::new(ManualClock::new()),
        pairing_ttl: Duration::from_secs(60),
        control_secret: control.clone(),
        instance: Secret::generate(CODE_BYTES).unwrap(),
        status: if has_engine {
            ready()
        } else {
            testing_hub::app::no_engine_status()
        },
        engine: engine.map(|e| e as Arc<dyn Engine>),
        ask,
        origins: Origins {
            state_dir: Some(Origin::Default),
            areas: Some(Origin::Setting),
            tasks_vault: Some(Origin::Env),
            ci,
        },
        watch,
        step: None,
        tasks_vault: None,
        ui: testing_hub::ui::UiFiles::none(),
        warm,
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

fn etag_of(reply: &appkit_testkit::wire::Reply) -> Option<String> {
    reply
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("etag"))
        .map(|(_, v)| v.clone())
}

#[tokio::test]
async fn fleet_is_the_document_of_the_engine_asked_in_the_export_form_with_the_places() {
    let engine = FakeEngine::new();
    let hub = start(Some(Arc::clone(&engine))).await;
    let r = get(&hub.server, &route("fleet"), Some(&hub.token)).await;
    assert_eq!(r.status, 200, "{r:?}");
    let want: Value = serde_json::from_str(FLEET).unwrap();
    assert_eq!(r.json(), want, "passed on as the engine wrote it");
    assert_eq!(
        etag_of(&r).as_deref(),
        Some(&*format!("\"{FLEET_DIGEST}\""))
    );
    let asked = engine.asked("fleet");
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0],
        Some(json!({
            "export": true,
            "state_dir": place("state").display().to_string(),
            "areas": place("areas.json").display().to_string(),
        }))
    );
}

#[tokio::test]
async fn a_client_that_holds_the_digest_gets_304() {
    let engine = FakeEngine::new();
    let hub = start(Some(engine)).await;
    let held = format!("\"{FLEET_DIGEST}\"");
    let r = get_with(
        &hub.server,
        &route("fleet"),
        Some(&hub.token),
        &[("If-None-Match", &held)],
    )
    .await;
    assert_eq!(r.status, 304, "{r:?}");
    let other = get_with(
        &hub.server,
        &route("fleet"),
        Some(&hub.token),
        &[("If-None-Match", "\"sha256:0000000000000000\"")],
    )
    .await;
    assert_eq!(other.status, 200, "{other:?}");
}

#[tokio::test]
async fn fleet_takes_no_query_and_without_an_engine_says_it_is_not_ready() {
    let engine = FakeEngine::new();
    let hub = start(Some(Arc::clone(&engine))).await;
    let q = get(
        &hub.server,
        &format!("{}?x=1", route("fleet")),
        Some(&hub.token),
    )
    .await;
    assert_eq!(q.status, 400, "{q:?}");
    assert!(
        engine.asked("fleet").is_empty(),
        "refused before the engine"
    );

    let none = start(None).await;
    for name in [
        "fleet",
        "events",
        "repo?key=beta.nvim-ac6d3a4ece7f",
        "finding?key=beta.nvim-ac6d3a4ece7f",
    ] {
        let r = get(&none.server, &route(name), Some(&none.token)).await;
        assert_eq!(r.status, 503, "{name}: {r:?}");
        assert_eq!(r.json()["error"]["code"], "engine_unavailable", "{name}");
        assert_eq!(r.json()["error"]["problem"], "no_engine", "{name}");
    }
}

/// A token that is in no golden document.
const TOKEN: &str = "github_pat_spy0123456789abcdefABCDEF";

#[tokio::test]
async fn a_token_goes_to_the_engine_in_the_requests_that_ask_github_and_comes_back_in_no_answer() {
    let engine = FakeEngine::new();
    let hub = start_asking(
        Some(Arc::clone(&engine)),
        Limits::default(),
        None,
        Ask {
            github_token: GithubToken::parse(TOKEN),
            ..ask()
        },
        CiState::Gh,
    )
    .await;

    let mut answers = Vec::new();
    for path in [
        route("fleet"),
        format!("{}?key={BETA}", route("repo")),
        format!("{}?key={BETA}", route("finding")),
        route("about"),
    ] {
        let reply = get(&hub.server, &path, Some(&hub.token)).await;
        assert_eq!(reply.status, 200, "{path}");
        assert!(!reply.body.contains(TOKEN), "{path}: {}", reply.body);
        answers.push(reply);
    }
    let about: Value = serde_json::from_str(&answers[3].body).unwrap();
    assert_eq!(about["ci"], "gh", "where it came from, as a word");

    for method in ["fleet", "repo"] {
        let asked = engine.asked(method);
        assert_eq!(asked.len(), 1, "{method}");
        let params = asked[0].as_ref().unwrap();
        assert_eq!(params["remote"], true, "{method}");
        assert_eq!(params["github_token"], TOKEN, "{method}");
        assert_eq!(params["remote_max_age"], 60, "{method}");
    }
    let finding = engine.asked("finding");
    let params = finding[0].as_ref().unwrap();
    assert!(params.get("github_token").is_none(), "{params}");
    assert!(params.get("remote").is_none(), "{params}");

    // A page that reads the fleet again does not start the engine, so it does not reach GitHub.
    let before = engine.asked("fleet").len();
    for _ in 0..5 {
        let reply = get(&hub.server, &route("fleet"), Some(&hub.token)).await;
        assert_eq!(reply.status, 200);
    }
    assert_eq!(
        engine.asked("fleet").len(),
        before,
        "no question per page view"
    );
}

#[tokio::test]
async fn an_engine_that_fails_does_not_bring_the_token_into_the_answer() {
    let engine = FakeEngine::new();
    *engine.repo.lock().unwrap() = RepoAnswer::Timeout;
    let hub = start_asking(
        Some(Arc::clone(&engine)),
        Limits::default(),
        None,
        Ask {
            github_token: GithubToken::parse(TOKEN),
            ..ask()
        },
        CiState::File,
    )
    .await;
    let reply = get(
        &hub.server,
        &format!("{}?key={BETA}", route("repo")),
        Some(&hub.token),
    )
    .await;
    assert_ne!(reply.status, 200);
    assert!(!reply.body.contains(TOKEN), "{}", reply.body);
}

#[tokio::test]
async fn without_a_token_nothing_asks_github_and_about_says_why() {
    let engine = FakeEngine::new();
    let hub = start(Some(Arc::clone(&engine))).await;
    let reply = get(&hub.server, &route("fleet"), Some(&hub.token)).await;
    assert_eq!(reply.status, 200);
    let asked = engine.asked("fleet");
    let params = asked[0].as_ref().unwrap();
    assert!(params.get("remote").is_none(), "{params}");
    assert!(params.get("github_token").is_none(), "{params}");
    let about: Value = serde_json::from_str(
        &get(&hub.server, &route("about"), Some(&hub.token))
            .await
            .body,
    )
    .unwrap();
    assert_eq!(about["ci"], "no_token");
}

#[tokio::test]
async fn repo_is_asked_by_its_key_and_has_its_digest_as_the_etag() {
    let engine = FakeEngine::new();
    let hub = start(Some(Arc::clone(&engine))).await;
    let r = get(
        &hub.server,
        &format!("{}?key={BETA}", route("repo")),
        Some(&hub.token),
    )
    .await;
    assert_eq!(r.status, 200, "{r:?}");
    assert_eq!(r.json()["repo"]["key"], BETA);
    assert_eq!(etag_of(&r).as_deref(), Some(&*format!("\"{REPO_DIGEST}\"")));
    let asked = engine.asked("repo");
    assert_eq!(asked.len(), 1);
    let params = asked[0].clone().unwrap();
    assert_eq!(params["repo"], BETA);
    assert_eq!(params["export"], true);
    assert_eq!(params["areas"], place("areas.json").display().to_string());
}

#[tokio::test]
async fn the_findings_are_one_document_asked_by_the_key_without_the_export_form() {
    let engine = FakeEngine::new();
    let hub = start(Some(Arc::clone(&engine))).await;
    let r = get(
        &hub.server,
        &format!("{}?key={BETA}", route("finding")),
        Some(&hub.token),
    )
    .await;
    assert_eq!(r.status, 200, "{r:?}");
    let doc = r.json();
    assert_eq!(doc["kind"], "findings");
    assert_eq!(doc["repo"], BETA);
    assert!(doc["findings"].as_array().is_some_and(|l| !l.is_empty()));
    assert_eq!(
        etag_of(&r),
        doc["digest"].as_str().map(|d| format!("\"{d}\""))
    );
    let asked = engine.asked("finding");
    assert_eq!(
        asked,
        [Some(json!({
            "repo": BETA,
            "state_dir": place("state").display().to_string(),
            "areas": place("areas.json").display().to_string(),
        }))]
    );
}

#[tokio::test]
async fn a_query_by_key_that_is_not_one_plain_key_never_reaches_the_engine() {
    let engine = FakeEngine::new();
    let hub = start(Some(Arc::clone(&engine))).await;
    for name in ["repo", "finding"] {
        for query in [
            "",
            "?key=",
            "?key=beta.nvim",
            "?key=..%2F..-0123456789ab",
            "?key=beta.nvim-ac6d3a4ece7f&key=beta.nvim-ac6d3a4ece7f",
            "?key=beta.nvim-ac6d3a4ece7f&state_dir=x",
            "?id=beta.nvim-ac6d3a4ece7f",
        ] {
            let r = get(
                &hub.server,
                &format!("{}{query}", route(name)),
                Some(&hub.token),
            )
            .await;
            assert_eq!(r.status, 400, "{name}{query}: {r:?}");
            assert_eq!(r.json()["error"]["code"], "bad_query", "{name}{query}");
        }
        assert!(engine.asked(name).is_empty(), "{:?}", engine.asked(name));
    }
}

#[tokio::test]
async fn a_key_the_engine_finds_nothing_for_is_404_and_a_failure_tells_no_detail() {
    let engine = FakeEngine::new();
    let hub = start(Some(Arc::clone(&engine))).await;
    let path = format!("{}?key=gone.nvim-0123456789ab", route("repo"));
    *engine.repo.lock().unwrap() = RepoAnswer::Exit2;
    let r = get(&hub.server, &path, Some(&hub.token)).await;
    assert_eq!(r.status, 404, "{r:?}");
    assert_eq!(r.json()["error"]["code"], "not_found");
    assert!(!r.body.contains("TESTING_REPOS_ROOT"), "{r:?}");
    *engine.repo.lock().unwrap() = RepoAnswer::Timeout;
    let t = get(&hub.server, &path, Some(&hub.token)).await;
    assert_eq!(t.status, 504, "{t:?}");
}

// The snapshot and the event stream.

/// A state folder with the folder of one project in it, and the path of its `last.json`.
fn state_folder(name: &str) -> (Scratch, PathBuf) {
    let dir = Scratch::new(&format!("live-{name}"));
    let project = dir.join("testing").join("alpha.nvim-0123456789ab");
    std::fs::create_dir_all(&project).unwrap();
    let last = project.join("last.json");
    (dir, last)
}

#[tokio::test]
async fn pages_that_read_the_fleet_ask_the_engine_once() {
    let engine = FakeEngine::new();
    let (dir, _) = state_folder("once");
    let hub = start_watching(&engine, &dir).await;
    for _ in 0..5 {
        let r = get(&hub.server, &route("fleet"), Some(&hub.token)).await;
        assert_eq!(r.status, 200, "{r:?}");
        assert_eq!(
            etag_of(&r).as_deref(),
            Some(&*format!("\"{FLEET_DIGEST}\"")),
            "the snapshot keeps the digest of the document as its ETag"
        );
    }
    assert_eq!(engine.asked("fleet").len(), 1);
}

#[tokio::test]
async fn the_first_read_of_the_fleet_starts_with_the_hub_not_with_the_first_page() {
    let engine = FakeEngine::new();
    let limits = Limits {
        snapshot_max_age: None,
        ..Limits::default()
    };
    let hub = start_warming(
        Some(Arc::clone(&engine)),
        limits,
        None,
        ask(),
        CiState::NoToken,
        true,
    )
    .await;
    // No page has asked yet: the hub asks by itself.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while engine.asked("fleet").is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the hub did not read the fleet at its start"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        engine.asked("fleet"),
        vec![Some(ask().fleet())],
        "with what every read carries"
    );
    // The first page finds it read.
    for _ in 0..3 {
        let r = get(&hub.server, &route("fleet"), Some(&hub.token)).await;
        assert_eq!(r.status, 200, "{r:?}");
    }
    assert_eq!(
        engine.asked("fleet").len(),
        1,
        "one read for the start and the pages"
    );
}

#[tokio::test]
async fn a_hub_without_an_engine_warms_nothing() {
    let hub = start_warming(None, Limits::default(), None, ask(), CiState::NoToken, true).await;
    let r = get(&hub.server, &route("fleet"), Some(&hub.token)).await;
    assert_ne!(r.status, 200, "{r:?}");
}

#[tokio::test]
async fn the_stream_says_hello_and_a_new_result_is_one_question_and_one_event() {
    let engine = FakeEngine::new();
    let (dir, last) = state_folder("event");
    let hub = start_watching(&engine, &dir).await;
    let mut stream = Stream::open(&hub.server, &route("events"), &hub.token).await;
    let hello = stream.until("event: hello").await;
    assert!(hello.starts_with("HTTP/1.1 200"), "{hello}");
    assert!(hello.contains("text/event-stream"), "{hello}");
    stream.until("\n\n").await;
    assert_eq!(
        engine.asked("fleet").len(),
        1,
        "the snapshot before the stream"
    );

    // A run writes its result: the engine now says something else about the fleet.
    engine.change_the_fleet();
    std::fs::write(&last, "{}").unwrap();
    let text = stream.until("event: fleet").await;
    assert!(text.contains(r#""cause":"fs""#), "{text}");
    assert!(text.contains("rev:777777"), "{text}");

    // What the page then reads comes from the snapshot, however often it reads.
    for _ in 0..3 {
        let r = get(&hub.server, &route("fleet"), Some(&hub.token)).await;
        assert_eq!(etag_of(&r).as_deref(), Some(&*format!("\"{NEW_DIGEST}\"")));
    }
    let held = format!("\"{NEW_DIGEST}\"");
    let r = get_with(
        &hub.server,
        &route("fleet"),
        Some(&hub.token),
        &[("If-None-Match", &held)],
    )
    .await;
    assert_eq!(r.status, 304, "{r:?}");
    assert_eq!(
        engine.asked("fleet").len(),
        2,
        "one more for the one change"
    );
}

#[tokio::test]
async fn what_is_not_a_result_wakes_nothing() {
    let engine = FakeEngine::new();
    let (dir, last) = state_folder("quiet");
    let hub = start_watching(&engine, &dir).await;
    let mut stream = Stream::open(&hub.server, &route("events"), &hub.token).await;
    stream.until("event: hello").await;
    let before = engine.asked("fleet").len();
    engine.change_the_fleet();
    let project = last.parent().unwrap();
    std::fs::write(project.join("prev.json"), "{}").unwrap();
    std::fs::write(project.join("runs.jsonl"), "{}\n").unwrap();
    std::fs::write(project.join("last.json.tmp"), "{}").unwrap();
    std::fs::write(dir.join("main.shada"), "x").unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(engine.asked("fleet").len(), before, "a scan was woken");
    assert!(!stream.seen.contains("event: fleet"), "{}", stream.seen);
}

#[tokio::test]
async fn a_scan_that_finds_the_same_fleet_tells_nothing() {
    let engine = FakeEngine::new();
    let (dir, last) = state_folder("same");
    let hub = start_watching(&engine, &dir).await;
    let mut stream = Stream::open(&hub.server, &route("events"), &hub.token).await;
    stream.until("event: hello").await;
    let before = engine.asked("fleet").len();
    std::fs::write(&last, "{}").unwrap();
    for _ in 0..50 {
        if engine.asked("fleet").len() > before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(engine.asked("fleet").len() > before, "the scan ran");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!stream.seen.contains("event: fleet"), "{}", stream.seen);
}

#[tokio::test]
async fn a_hub_that_cannot_watch_still_starts_and_still_answers() {
    let engine = FakeEngine::new();
    let dir = Scratch::new("live-none");
    let hub = start_watching(&engine, &dir.join("no-such-folder")).await;
    let r = get(&hub.server, &route("fleet"), Some(&hub.token)).await;
    assert_eq!(r.status, 200, "{r:?}");
}

#[tokio::test]
async fn the_stream_takes_no_query_and_a_session_opens_only_so_many() {
    let engine = FakeEngine::new();
    let hub = start(Some(engine)).await;
    let q = get(
        &hub.server,
        &format!("{}?x=1", route("events")),
        Some(&hub.token),
    )
    .await;
    assert_eq!(q.status, 400, "{q:?}");
    let mut open = Vec::new();
    for _ in 0..Limits::default().max_streams_per_session {
        let mut stream = Stream::open(&hub.server, &route("events"), &hub.token).await;
        stream.until("event: hello").await;
        open.push(stream);
    }
    let more = get(&hub.server, &route("events"), Some(&hub.token)).await;
    assert_eq!(more.status, 429, "{more:?}");
    assert_eq!(more.json()["error"]["code"], "too_many_streams");
}

#[tokio::test]
async fn about_names_the_engine_and_where_the_hub_reads_as_words_not_paths() {
    let engine = FakeEngine::new();
    let hub = start(Some(engine)).await;
    let r = get(&hub.server, &route("about"), Some(&hub.token)).await;
    assert_eq!(r.status, 200, "{r:?}");
    let doc = r.json();
    let hello: Value = serde_json::from_str(HELLO).unwrap();
    assert_eq!(doc["engine"], hello["engine"]);
    assert!(doc["engine_problem"].is_null());
    assert_eq!(
        doc["reads"],
        json!({ "state_dir": "default", "areas": "setting", "tasks_vault": "environment" })
    );
    // The body is JSON: a backslash of a Windows path would be doubled in it.
    let body = r.body.replace("\\\\", "/");
    for p in [place("state"), place("areas.json")] {
        let shown = p.display().to_string().replace('\\', "/");
        assert!(!body.contains(&shown), "no path of the device: {r:?}");
    }
}
