//! The read routes of the hub against an engine in the process, without Neovim: a fake that answers
//! with the golden documents of testing.nvim (`tests/fixtures/golden`, copies of
//! `TESTS/golden/testing-export-1/` and `TESTS/golden/testing-export-1-hello/` at the commit of
//! `engine.lock`) and keeps what it was asked.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use appkit_core::authn::{ManualClock, Secret, CODE_BYTES, TOKEN_BYTES};
use appkit_hub::engine::{CallFuture, Engine, EngineError, Method, Reply};
use appkit_hub::{HubStatus, Limits, RunningServer};
use serde_json::{json, Value};
use testing_hub::engine::Ask;
use testing_hub::settings::Origin;
use testing_hub::Origins;

mod common;

use common::{get, get_with, session};

const HELLO: &str = include_str!("fixtures/golden/hello.json");
const FLEET: &str = include_str!("fixtures/golden/fleet.json");
const REPO: &str = include_str!("fixtures/golden/repo.json");

/// The digests of the golden documents: their ETags.
const FLEET_DIGEST: &str = "sha256:e4259770b57246bc";
const REPO_DIGEST: &str = "sha256:83f9e002d7020270";
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

/// An engine in the process. It answers `hello`, `fleet` and `repo` with the golden documents and
/// keeps every question.
struct FakeEngine {
    repo: Mutex<RepoAnswer>,
    asked: Mutex<Vec<(&'static str, Option<Value>)>>,
}

impl FakeEngine {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            repo: Mutex::new(RepoAnswer::Golden),
            asked: Mutex::new(Vec::new()),
        })
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
                "hello" => HELLO,
                "fleet" => FLEET,
                "repo" => match *self.repo.lock().unwrap() {
                    RepoAnswer::Golden => REPO,
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
    }
}

async fn start(engine: Option<Arc<FakeEngine>>) -> Hub {
    let control = Secret::generate(TOKEN_BYTES).unwrap();
    let has_engine = engine.is_some();
    let server = testing_hub::start(testing_hub::Start {
        addr: "127.0.0.1:0".parse().unwrap(),
        limits: Limits::default(),
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
        ask: ask(),
        origins: Origins {
            state_dir: Some(Origin::Default),
            areas: Some(Origin::Setting),
        },
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
    for name in ["fleet", "repo?key=beta.nvim-ac6d3a4ece7f"] {
        let r = get(&none.server, &route(name), Some(&none.token)).await;
        assert_eq!(r.status, 503, "{name}: {r:?}");
        assert_eq!(r.json()["error"]["code"], "engine_unavailable", "{name}");
        assert_eq!(r.json()["error"]["problem"], "no_engine", "{name}");
    }
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
async fn a_query_of_repo_that_is_not_one_plain_key_never_reaches_the_engine() {
    let engine = FakeEngine::new();
    let hub = start(Some(Arc::clone(&engine))).await;
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
            &format!("{}{query}", route("repo")),
            Some(&hub.token),
        )
        .await;
        assert_eq!(r.status, 400, "{query}: {r:?}");
        assert_eq!(r.json()["error"]["code"], "bad_query", "{query}");
    }
    assert!(
        engine.asked("repo").is_empty(),
        "{:?}",
        engine.asked("repo")
    );
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
        json!({ "state_dir": "default", "areas": "setting" })
    );
    // The body is JSON: a backslash of a Windows path would be doubled in it.
    let body = r.body.replace("\\\\", "/");
    for p in [place("state"), place("areas.json")] {
        let shown = p.display().to_string().replace('\\', "/");
        assert!(!body.contains(&shown), "no path of the device: {r:?}");
    }
}
