//! The release assistant for the runner guard (`release/*`), against a stand-in for the engine's `testing ci release
//! ... --json` (`tests/fixtures/fake-release.lua`, run by the real Neovim as the hub runs the engine: `TESTING_NVIM`,
//! else the `nvim` on the `PATH`; without one the tests say so and pass, unless `TESTING_HUB_REQUIRE_ENGINE=1`). The
//! stand-in writes what it was asked into a folder of the test, and a release into `released.txt` there: what is
//! asserted is what the hub asked of the engine, and that nothing is released the engine was not asked to release.

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

/// A repository on the stand-in's list of reviewed commits, and one of the golden fleet whose CI is red.
const LISTED: &str = "fixture-owner/alpha.nvim";
const RED: &str = "fixture-owner/beta.nvim";
const S1: &str = "1111111111111111111111111111111111111111";
const S2: &str = "2222222222222222222222222222222222222222";

/// `hello` and `fleet` as the golden documents; nothing else is asked of it.
struct FleetEngine;

impl Engine for FleetEngine {
    fn call<'a>(&'a self, method: Method, _params: Option<&'a Value>) -> CallFuture<'a> {
        Box::pin(async move {
            let text = match method.as_str() {
                "hello" => HELLO,
                "fleet" => FLEET,
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

/// The folder the stand-in writes into, and the program that runs it.
struct Fixture {
    dir: Scratch,
}

impl Fixture {
    fn new() -> Self {
        Self {
            dir: Scratch::new("release"),
        }
    }

    fn program_or_skip(&self) -> Option<StepProgram> {
        match nvim() {
            Ok(nvim) => {
                let script = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests")
                    .join("fixtures")
                    .join("fake-release.lua");
                let mut env: Vec<(OsString, OsString)> =
                    ["PATH", "SystemRoot", "TEMP", "TMP", "HOME", "USERPROFILE"]
                        .iter()
                        .filter_map(|n| std::env::var_os(n).map(|v| (OsString::from(n), v)))
                        .collect();
                env.push((
                    OsString::from("FAKE_RELEASE_DIR"),
                    self.dir.as_os_str().into(),
                ));
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

    fn lines(&self, name: &str) -> Vec<String> {
        std::fs::read_to_string(self.dir.join(name))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// What the stand-in was asked, one line per call, without the two words every call begins with.
    fn asked(&self) -> Vec<String> {
        self.lines("argv.txt")
            .into_iter()
            .map(|line| {
                line.strip_prefix("ci release ")
                    .unwrap_or_else(|| panic!("not a step of ci release: {line}"))
                    .to_owned()
            })
            .collect()
    }

    /// What the stand-in released: `<repo> <sha>` per release.
    fn released(&self) -> Vec<String> {
        self.lines("released.txt")
    }

    fn mode(&self, mode: &str) {
        std::fs::write(self.dir.join("mode.txt"), mode).unwrap();
    }
}

struct Hub {
    server: RunningServer,
    token: String,
}

async fn start_hub(program: Option<StepProgram>) -> Hub {
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
        engine: Some(Arc::new(FleetEngine) as Arc<dyn Engine>),
        ask: Ask::default(),
        origins: Origins::default(),
        watch: None,
        step: program,
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
    async fn get(&self, path: &str) -> appkit_testkit::wire::Reply {
        get(&self.server, &route(path), Some(&self.token)).await
    }

    async fn release(&self, body: &Value) -> appkit_testkit::wire::Reply {
        post(
            &self.server,
            &route("release"),
            Some(&self.token),
            &body.to_string(),
        )
        .await
    }

    /// The first step, which says which repositories a later step may name.
    async fn repos(&self) -> Value {
        let r = self.get("release/repos").await;
        assert_eq!(r.status, 200, "{r:?}");
        r.json()
    }

    /// Where the release that was started last stands, once it is not pending any more.
    async fn outcome(&self) -> Value {
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            let r = self.get("release/outcome").await;
            assert_eq!(r.status, 200, "{r:?}");
            let doc = r.json();
            if doc["state"] != "pending" {
                return doc;
            }
            assert!(Instant::now() < end, "the release did not end: {doc}");
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }

    async fn show(&self, repo: &str, sha: &str) -> appkit_testkit::wire::Reply {
        self.get(&format!("release/show?repo={repo}&sha={sha}"))
            .await
    }
}

fn code(reply: &appkit_testkit::wire::Reply) -> String {
    reply.json()["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[tokio::test]
async fn the_steps_are_the_engines_and_a_commit_is_released_only_after_its_review_aid() {
    let fx = Fixture::new();
    let Some(program) = fx.program_or_skip() else {
        return;
    };
    let hub = start_hub(Some(program)).await;

    // 1. The repositories: the engine's list and the fleet, each once, the red one first.
    let doc = hub.repos().await;
    assert_eq!(doc["kind"], "hub.release.repos");
    assert_eq!(doc["exit"], 0);
    let names: Vec<&str> = doc["repos"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["name"].as_str())
        .collect();
    assert_eq!(names, [RED, LISTED, "elsewhere/tool"], "{doc}");
    assert_eq!(doc["repos"][0]["red"], true);
    assert_eq!(doc["repos"][0]["listed"], false);
    assert_eq!(
        doc["repos"][1],
        json!({ "name": LISTED, "listed": true, "mode": "auto", "reviewed": 2, "ci": "success", "red": false })
    );

    // 2. The commits, each with whether it is released; what is no full id cannot be chosen.
    let r = hub.get(&format!("release/commits?repo={RED}")).await;
    assert_eq!(r.status, 200, "{r:?}");
    let doc = r.json();
    assert_eq!(doc["kind"], "hub.release.commits");
    assert_eq!(doc["repo"], RED);
    let commits = doc["commits"].as_array().unwrap();
    assert_eq!(commits.len(), 2, "{doc}");
    assert_eq!(commits[0]["sha"], S2);
    assert_eq!(commits[0]["released"], false);
    assert_eq!(commits[1]["released"], true);

    // Not yet: nobody was shown what the commit changes.
    let r = hub.release(&json!({ "repo": RED, "sha": S2 })).await;
    assert_eq!(r.status, 409, "{r:?}");
    assert_eq!(code(&r), "not_shown");
    assert!(fx.released().is_empty());

    // 3. The review aid, with the file that deserves a close look.
    let r = hub.show(RED, S2).await;
    assert_eq!(r.status, 200, "{r:?}");
    let doc = r.json();
    assert_eq!(doc["kind"], "hub.release.show");
    assert_eq!(doc["sha"], S2);
    assert_eq!(doc["show"]["prev"], S1);
    assert_eq!(doc["show"]["files"][1]["flag"], "workflow");

    // The aid of one commit allows the release of no other, and of no other repository.
    for (repo, sha) in [(RED, S1), (LISTED, S2)] {
        let r = hub.release(&json!({ "repo": repo, "sha": sha })).await;
        assert_eq!(r.status, 409, "{repo} {sha}: {r:?}");
        assert_eq!(code(&r), "not_shown");
    }
    assert!(fx.released().is_empty());

    // 4. The release of exactly that commit is started; its outcome names the runs that started again.
    assert_eq!(hub.get("release/outcome").await.json()["state"], "none");
    let r = hub.release(&json!({ "repo": RED, "sha": S2 })).await;
    assert_eq!(r.status, 202, "{r:?}");
    assert_eq!(
        r.json(),
        json!({ "schema": 1, "kind": "hub.release.started", "repo": RED, "sha": S2 })
    );
    let doc = hub.outcome().await;
    assert_eq!(doc["kind"], "hub.release.outcome");
    assert_eq!(doc["state"], "done", "{doc}");
    assert_eq!((&doc["repo"], &doc["sha"]), (&json!(RED), &json!(S2)));
    assert_eq!(doc["exit"], 0);
    assert_eq!(doc["release"]["recorded"], true);
    assert_eq!(doc["release"]["reruns"][0]["id"], 71);
    assert_eq!(fx.released(), [format!("{RED} {S2}")]);

    // The aid is spent: the same request again releases nothing.
    let r = hub.release(&json!({ "repo": RED, "sha": S2 })).await;
    assert_eq!(code(&r), "not_shown", "{r:?}");
    assert_eq!(fx.released().len(), 1);

    // What the engine was asked, in order: fixed words, the repository, the full id, and always the document.
    assert_eq!(
        fx.asked(),
        [
            "--repos --json".to_owned(),
            format!("{RED} --commits --json"),
            format!("{RED} {S2} --show --json"),
            format!("{RED} {S2} --yes --json"),
        ]
    );
    // It ran outside a checkout, and with no token and no trust of a root in its environment.
    for env in fx.lines("env.txt") {
        assert_eq!(env, "FAKE_RELEASE_DIR", "the variables of the child");
    }
    hub.server.shutdown().await;
}

#[tokio::test]
async fn a_request_names_a_repository_the_assistant_offered_and_a_full_commit_id() {
    let fx = Fixture::new();
    let Some(program) = fx.program_or_skip() else {
        return;
    };
    let hub = start_hub(Some(program)).await;

    // Before the first step no repository was offered: nothing can be named.
    let r = hub.get(&format!("release/commits?repo={RED}")).await;
    assert_eq!(
        (r.status, code(&r).as_str()),
        (404, "unknown_repo"),
        "{r:?}"
    );
    hub.repos().await;
    let asked = fx.asked().len();

    // A repository that is none, or one the assistant did not offer.
    for (query, status, want) in [
        ("release/commits", 400, "bad_repo"),
        // The kit refuses a value that is no plain text before the route sees it.
        ("release/commits?repo=", 400, "bad_query"),
        ("release/commits?repo=alpha", 400, "bad_repo"),
        ("release/commits?repo=--yes/x", 400, "bad_repo"),
        ("release/commits?repo=o/r/x", 400, "bad_repo"),
        ("release/commits?repo=someone/else", 404, "unknown_repo"),
        (
            "release/commits?repo=fixture-owner/alpha.nvim&count=50",
            400,
            "bad_query",
        ),
        (
            "release/show?repo=someone/else&sha=2222222222222222222222222222222222222222",
            404,
            "unknown_repo",
        ),
    ] {
        let r = hub.get(query).await;
        assert_eq!(
            (r.status, code(&r).as_str()),
            (status, want),
            "{query}: {r:?}"
        );
    }
    // A commit id that is not the full one: a short id, a branch, an option, upper case.
    for sha in [
        "2222222",
        "main",
        "HEAD",
        "--yes",
        "",
        &S2.to_uppercase().replace('2', "A"),
    ] {
        let r = hub.show(RED, sha).await;
        assert_eq!(r.status, 400, "{sha:?}: {r:?}");
        assert!(
            ["bad_sha", "bad_query"].contains(&code(&r).as_str()),
            "{sha:?}: {r:?}"
        );
        let r = hub.release(&json!({ "repo": RED, "sha": sha })).await;
        assert_eq!(
            (r.status, code(&r).as_str()),
            (400, "bad_sha"),
            "{sha:?}: {r:?}"
        );
    }
    // A body with anything else in it, or of another shape.
    for body in [
        json!({ "repo": RED, "sha": S2, "note": "x" }),
        json!({ "repo": RED, "sha": S2, "yes": true }),
        json!({ "repo": RED, "sha": S2, "runners": "/tmp" }),
        json!({ "repo": [RED], "sha": S2 }),
        json!({ "sha": S2 }),
        json!([RED, S2]),
    ] {
        let r = hub.release(&body).await;
        assert_eq!(r.status, 400, "{body}: {r:?}");
    }
    // The name of a repository has no case on GitHub: the offered one in another case is the offered one.
    let r = hub
        .get("release/commits?repo=Fixture-Owner/Beta.nvim")
        .await;
    assert_eq!(r.status, 200, "{r:?}");
    // None of the refused requests reached the engine, and nothing was released.
    assert_eq!(fx.asked().len(), asked + 1, "{:?}", fx.asked());
    assert!(fx.released().is_empty());
    hub.server.shutdown().await;
}

#[tokio::test]
async fn a_review_aid_the_engine_did_not_give_for_that_commit_allows_no_release() {
    let fx = Fixture::new();
    let Some(program) = fx.program_or_skip() else {
        return;
    };
    let hub = start_hub(Some(program)).await;
    hub.repos().await;

    // GitHub has no such commit in this repository: the engine says so, and there is no aid.
    let gone = "e".repeat(40);
    let r = hub.show(RED, &gone).await;
    assert_eq!(r.status, 200, "{r:?}");
    let doc = r.json();
    assert_eq!(doc["exit"], 3);
    assert!(
        doc["problem"].as_str().unwrap().contains("has no commit"),
        "{doc}"
    );
    assert_eq!(doc["show"], Value::Null);
    let r = hub.release(&json!({ "repo": RED, "sha": gone })).await;
    assert_eq!(code(&r), "not_shown", "{r:?}");

    // The engine answers for another commit than the one asked: that is no aid of the commit asked.
    fx.mode("moved");
    let r = hub.show(RED, S2).await;
    assert_eq!(r.json()["show"], Value::Null, "{r:?}");
    let r = hub.release(&json!({ "repo": RED, "sha": S2 })).await;
    assert_eq!(code(&r), "not_shown", "{r:?}");
    let r = hub
        .release(&json!({ "repo": RED, "sha": "9".repeat(40) }))
        .await;
    assert_eq!(code(&r), "not_shown", "{r:?}");

    // A step the engine could not do is its answer, not a failure of the hub.
    fx.mode("");
    let r = hub.get("release/commits?repo=elsewhere/tool").await;
    assert_eq!(r.status, 200, "{r:?}");
    assert_eq!(r.json()["exit"], 3);
    assert_eq!(r.json()["commits"], json!([]));
    assert!(fx.released().is_empty());
    hub.server.shutdown().await;
}

#[tokio::test]
async fn a_release_whose_rerun_failed_says_that_it_is_recorded_and_what_failed() {
    let fx = Fixture::new();
    let Some(program) = fx.program_or_skip() else {
        return;
    };
    let hub = start_hub(Some(program)).await;
    hub.repos().await;
    assert_eq!(hub.show(RED, S2).await.status, 200);
    fx.mode("rerun-red");
    let r = hub.release(&json!({ "repo": RED, "sha": S2 })).await;
    assert_eq!(r.status, 202, "{r:?}");
    let doc = hub.outcome().await;
    assert_eq!(doc["state"], "done", "{doc}");
    assert_eq!(doc["exit"], 3);
    assert_eq!(doc["problem"], "re-run 71 failed: HTTP 403");
    assert_eq!(doc["release"]["recorded"], true);
    assert_eq!(doc["release"]["reruns"][0]["ok"], false);
    hub.server.shutdown().await;
}

/// A release is not tied to the request that started it: the answer comes at once, the release runs on, and until it
/// ended no other step is taken. (The hub gives a request 30 seconds; a release that re-runs many runs takes longer,
/// and a request cut off must not end it half way.)
#[tokio::test]
async fn a_release_runs_to_its_end_in_the_background_and_its_outcome_is_read_later() {
    let fx = Fixture::new();
    let Some(program) = fx.program_or_skip() else {
        return;
    };
    let hub = start_hub(Some(program)).await;
    hub.repos().await;
    assert_eq!(hub.show(RED, S2).await.status, 200);
    fx.mode("wait");
    let started = Instant::now();
    let r = hub.release(&json!({ "repo": RED, "sha": S2 })).await;
    assert_eq!(r.status, 202, "{r:?}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the answer does not wait for the release"
    );
    let r = hub.get("release/outcome").await.json();
    assert_eq!(r["state"], "pending", "{r}");
    assert_eq!((&r["repo"], &r["sha"]), (&json!(RED), &json!(S2)));
    // While it runs: no other step, and no second release (whose review aid would not be spent by the refusal).
    let r = hub.get("release/repos").await;
    assert_eq!((r.status, code(&r).as_str()), (409, "busy"), "{r:?}");
    let r = hub.release(&json!({ "repo": RED, "sha": S2 })).await;
    assert_eq!((r.status, code(&r).as_str()), (409, "busy"), "{r:?}");
    assert!(fx.released().is_empty(), "the stand-in still waits");
    std::fs::write(fx.dir.join("go"), "").unwrap();
    let doc = hub.outcome().await;
    assert_eq!(doc["state"], "done", "{doc}");
    assert_eq!(doc["release"]["recorded"], true);
    assert_eq!(fx.released(), [format!("{RED} {S2}")]);
    // It is over: the next step is taken again, and the outcome stays to be read.
    fx.mode("");
    assert_eq!(hub.get("release/repos").await.status, 200);
    assert_eq!(hub.get("release/outcome").await.json()["state"], "done");
    hub.server.shutdown().await;
}

/// A release whose step gives no document (an engine that does not know it) is an outcome that says so.
#[tokio::test]
async fn a_release_the_engine_gives_no_document_for_is_a_failed_outcome() {
    let fx = Fixture::new();
    let Some(program) = fx.program_or_skip() else {
        return;
    };
    let hub = start_hub(Some(program)).await;
    hub.repos().await;
    assert_eq!(hub.show(RED, S2).await.status, 200);
    fx.mode("old");
    let r = hub.release(&json!({ "repo": RED, "sha": S2 })).await;
    assert_eq!(r.status, 202, "{r:?}");
    let doc = hub.outcome().await;
    assert_eq!(doc["state"], "failed", "{doc}");
    assert_eq!(doc["error"]["code"], "engine_protocol");
    assert!(doc.get("release").is_none(), "{doc}");
    assert!(fx.released().is_empty());
    hub.server.shutdown().await;
}

#[tokio::test]
async fn an_engine_without_the_steps_and_a_second_step_at_once_are_said_as_such() {
    let fx = Fixture::new();
    let Some(program) = fx.program_or_skip() else {
        return;
    };
    let hub = Arc::new(start_hub(Some(program)).await);

    // An engine that does not know the step prints no document.
    fx.mode("old");
    let r = hub.get("release/repos").await;
    assert_eq!(
        (r.status, code(&r).as_str()),
        (502, "engine_protocol"),
        "{r:?}"
    );

    // One step at a time: while one waits, another is refused, and goes through afterwards.
    fx.mode("wait");
    let first = {
        let hub = Arc::clone(&hub);
        tokio::spawn(async move { hub.get("release/repos").await })
    };
    let end = Instant::now() + Duration::from_secs(20);
    while fx.asked().len() < 2 {
        assert!(Instant::now() < end, "the first step did not start");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let r = hub.get("release/repos").await;
    assert_eq!((r.status, code(&r).as_str()), (409, "busy"), "{r:?}");
    std::fs::write(fx.dir.join("go"), "").unwrap();
    let first = first.await.unwrap();
    assert_eq!(first.status, 200, "{first:?}");
    assert_eq!(hub.get("release/repos").await.status, 200);
    let hub = Arc::into_inner(hub).expect("the first request is done");
    hub.server.shutdown().await;
}

#[tokio::test]
async fn without_an_engine_no_step_is_there_and_without_a_session_no_route() {
    let hub = start_hub(None).await;
    for path in [
        "release/repos",
        "release/commits?repo=o/r",
        "release/show?repo=o/r&sha=2222222222222222222222222222222222222222",
    ] {
        let r = hub.get(path).await;
        assert_eq!(
            (r.status, code(&r).as_str()),
            (503, "release_unavailable"),
            "{path}: {r:?}"
        );
        let r = get(&hub.server, &route(path), None).await;
        assert_eq!(r.status, 401, "{path} without a session: {r:?}");
    }
    let body = json!({ "repo": "o/r", "sha": S2 }).to_string();
    let r = hub.release(&json!({ "repo": "o/r", "sha": S2 })).await;
    assert_eq!(
        (r.status, code(&r).as_str()),
        (503, "release_unavailable"),
        "{r:?}"
    );
    let r = post(&hub.server, &route("release"), None, &body).await;
    assert_eq!(r.status, 401, "{r:?}");
    // No release was ever started: the outcome says so, to a session only.
    assert_eq!(hub.get("release/outcome").await.json()["state"], "none");
    let r = get(&hub.server, &route("release/outcome"), None).await;
    assert_eq!(r.status, 401, "{r:?}");
    hub.server.shutdown().await;
}
