//! The hub against the real testing.nvim: a synthetic repositories root (three repositories with
//! neutral names), a data classes file of the test's own and an empty state folder, never the data
//! of the machine. The checkout is `TESTING_ENGINE_DIR`, else `$REPOS_DIR/testing.nvim`; Neovim is
//! `TESTING_NVIM`, else the one on the `PATH`. Without them the test says so and passes, unless
//! `TESTING_HUB_REQUIRE_ENGINE=1` (CI), where a missing engine is a failure.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use appkit_core::authn::{ManualClock, Secret, CODE_BYTES, TOKEN_BYTES};
use appkit_hub::config::FileConfig;
use appkit_hub::discovery::{discover, AppInputs};
use appkit_hub::env::Env;
use appkit_hub::Limits;
use appkit_scratch::Scratch;
use serde_json::Value;
use testing_hub::engine::{
    cli_config, probe, Ask, Calls, CliEngine, Engine, EngineError, DISCOVERY,
};
use testing_hub::settings::TestingSettings;
use testing_hub::store::RefreshOptions;
use testing_hub::{Origins, Watch};

mod common;

use common::{get, session, Stream};

/// The repositories of the root, with the class each has in the data classes of the test.
const REPOS: [(&str, &str); 3] = [
    ("alpha.nvim", "public"),
    ("beta.nvim", "internal"),
    ("secret.nvim", "private"),
];

struct Fixture {
    // Declared first: dropped last, after the engine is done with the files.
    dir: Scratch,
}

impl Fixture {
    fn new() -> Self {
        let dir = Scratch::new("engine");
        for (name, _) in REPOS {
            let repo = dir.join("root").join(name);
            std::fs::create_dir_all(&repo).unwrap();
            std::fs::write(repo.join(".testing.lua"), "return {}\n").unwrap();
        }
        std::fs::create_dir_all(dir.join("state")).unwrap();
        let areas: serde_json::Map<String, Value> = REPOS
            .iter()
            .map(|(name, class)| {
                (
                    (*name).to_owned(),
                    serde_json::json!({ "class": class, "repo": name }),
                )
            })
            .collect();
        std::fs::write(
            dir.join("areas.json"),
            serde_json::json!({ "schema": 1, "areas": areas }).to_string(),
        )
        .unwrap();
        Self { dir }
    }

    fn root(&self) -> std::path::PathBuf {
        self.dir.join("root")
    }

    fn ask(&self) -> Ask {
        Ask {
            state_dir: Some(self.dir.join("state")),
            areas: Some(self.dir.join("areas.json")),
            ..Ask::default()
        }
    }
}

/// The engine of this machine for the fixture's root, or why there is none.
async fn engine(root: &Path) -> Result<CliEngine, String> {
    let env = Env::from_process();
    let file = FileConfig::<TestingSettings> {
        vault: Some(root.to_path_buf()),
        ..FileConfig::default()
    };
    let setup = discover(&DISCOVERY, &file, AppInputs::default(), &env)
        .await
        .map_err(|p| p.to_string())?;
    Ok(CliEngine::new(cli_config(&setup, env)))
}

/// The engine, or `None` after saying why it is missing (a failure in CI).
async fn engine_or_skip(root: &Path) -> Option<CliEngine> {
    match engine(root).await {
        Ok(engine) => Some(engine),
        Err(why) => {
            assert!(
                std::env::var_os("TESTING_HUB_REQUIRE_ENGINE").is_none_or(|v| v != "1"),
                "TESTING_HUB_REQUIRE_ENGINE=1 and no engine: {why}"
            );
            eprintln!("skipped: no testing.nvim engine ({why})");
            None
        }
    }
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// The real engine takes the token on stdin and says that it does. The repositories of the root have no remote, so no
/// request goes out (no network in a test); the answers of GitHub are recorded in the specs of testing.nvim.
#[tokio::test]
async fn the_real_engine_takes_a_token_in_the_request_and_writes_it_nowhere() {
    const TOKEN: &str = "github_pat_spy0123456789abcdefABCDEF";
    let fx = Fixture::new();
    let Some(engine) = engine_or_skip(&fx.root()).await else {
        return;
    };
    let info = probe(&engine).await.expect("hello");
    assert!(
        info.capabilities
            .iter()
            .any(|c| c == testing_hub::github::REMOTE_CAPABILITY),
        "{:?}",
        info.capabilities
    );
    let ask = Ask {
        github_token: testing_hub::github::GithubToken::parse(TOKEN),
        ..fx.ask()
    };
    let reply = engine
        .call(Calls::Fleet, Some(&ask.fleet()))
        .await
        .expect("fleet with a token");
    assert!(!reply.text.contains(TOKEN));
    let doc = json(&reply.text);
    assert_eq!(doc["remote"]["asked"], true, "{doc}");
    assert!(
        doc["repos"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["ci_remote"].is_object()),
        "every row has the record of the adapter: {doc}"
    );
    // Nothing of it in the state folder either: the cache of the adapter holds answers, not the token.
    fn holds(dir: &Path, needle: &str) -> bool {
        std::fs::read_dir(dir).unwrap().flatten().any(|e| {
            let path = e.path();
            if path.is_dir() {
                holds(&path, needle)
            } else {
                std::fs::read_to_string(&path).is_ok_and(|t| t.contains(needle))
            }
        })
    }
    assert!(!holds(&fx.dir.join("state"), TOKEN));
}

/// The child holds the token in its memory while the CI adapter reads the `ci_remote` section of each repository's
/// `.testing.lua`. That file is Lua: it must not run in the child of the hub, whatever a repository put there.
#[tokio::test]
async fn a_request_with_a_token_runs_no_code_of_a_repository() {
    let fx = Fixture::new();
    let Some(engine) = engine_or_skip(&fx.root()).await else {
        return;
    };
    let marker = fx.dir.join("the-repository-ran");
    let lua = marker.display().to_string().replace('\\', "/");
    std::fs::write(
        fx.root().join("alpha.nvim").join(".testing.lua"),
        format!("io.open([[{lua}]], 'w'):close()\nreturn {{}}\n"),
    )
    .unwrap();
    let ask = Ask {
        github_token: testing_hub::github::GithubToken::parse(
            "github_pat_spy0123456789abcdefABCDEF",
        ),
        ..fx.ask()
    };
    engine
        .call(Calls::Fleet, Some(&ask.fleet()))
        .await
        .expect("fleet with a token");
    assert!(!marker.exists(), "the .testing.lua of a repository ran");
}

#[tokio::test]
async fn the_real_engine_answers_hello_and_the_fleet_of_the_root_in_the_export_form() {
    let fx = Fixture::new();
    let Some(engine) = engine_or_skip(&fx.root()).await else {
        return;
    };
    let info = probe(&engine)
        .await
        .expect("hello answers as the hub needs");
    assert_eq!(info.schemas, vec![1]);

    let reply = engine
        .call(Calls::Fleet, Some(&fx.ask().fleet()))
        .await
        .expect("fleet");
    let doc = json(&reply.text);
    assert_eq!(doc["kind"], "fleet");
    assert_eq!(doc["form"], "export");
    let names: Vec<&str> = doc["repos"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["name"].as_str())
        .collect();
    assert_eq!(
        names,
        ["alpha.nvim", "beta.nvim"],
        "the private one is left out"
    );
    assert_eq!(doc["excluded_by"]["private"], 1, "{doc}");
    // The export form names no folder of this machine.
    let shown = fx.dir.display().to_string().replace('\\', "/");
    assert!(
        !reply.text.replace("\\\\", "/").contains(&shown),
        "a path of the fixture in the fleet"
    );

    // Without the data classes the child has none of its own: nothing is released.
    let closed = engine
        .call(
            Calls::Fleet,
            Some(
                &Ask {
                    areas: None,
                    ..fx.ask()
                }
                .fleet(),
            ),
        )
        .await
        .expect("fleet without areas");
    assert_eq!(json(&closed.text)["repos"], serde_json::json!([]));
}

#[tokio::test]
async fn the_real_engine_answers_a_repository_by_its_key_and_refuses_a_key_it_has_not() {
    let fx = Fixture::new();
    let Some(engine) = engine_or_skip(&fx.root()).await else {
        return;
    };
    let fleet = engine
        .call(Calls::Fleet, Some(&fx.ask().fleet()))
        .await
        .expect("fleet");
    let key = json(&fleet.text)["repos"][0]["key"]
        .as_str()
        .unwrap()
        .to_owned();
    let repo = engine
        .call(Calls::Repo, Some(&fx.ask().repo(&key)))
        .await
        .expect("repo");
    let doc = json(&repo.text);
    assert_eq!(doc["kind"], "repo");
    assert_eq!(doc["repo"]["key"], key.as_str());
    match engine
        .call(Calls::Repo, Some(&fx.ask().repo("gone.nvim-0123456789ab")))
        .await
    {
        Err(EngineError::Failed { code: Some(2), .. }) => {}
        other => panic!("a key without a repository: {other:?}"),
    }
}

#[tokio::test]
async fn the_hub_serves_the_fleet_of_the_real_engine() {
    let fx = Fixture::new();
    let Some(engine) = engine_or_skip(&fx.root()).await else {
        return;
    };
    let info = probe(&engine).await.expect("hello");
    let control = Secret::generate(TOKEN_BYTES).unwrap();
    let server = testing_hub::start(testing_hub::Start {
        addr: "127.0.0.1:0".parse().unwrap(),
        limits: Limits::default(),
        clock: Arc::new(ManualClock::new()),
        pairing_ttl: Duration::from_secs(60),
        control_secret: control.clone(),
        instance: Secret::generate(CODE_BYTES).unwrap(),
        status: appkit_hub::HubStatus {
            engine_ready: true,
            problem: None,
            capabilities: info.capabilities,
        },
        engine: Some(Arc::new(engine) as Arc<dyn Engine>),
        ask: fx.ask(),
        origins: Origins::default(),
        watch: None,
        ui: testing_hub::ui::UiFiles::none(),
    })
    .await
    .expect("the hub starts");
    let token = session(&server, &control).await;
    let r = get(&server, &testing_hub::IDENTITY.route("fleet"), Some(&token)).await;
    assert_eq!(r.status, 200, "{r:?}");
    assert_eq!(r.json()["repos"].as_array().map(Vec::len), Some(2));
    assert!(
        r.header("etag").is_some_and(|e| e.starts_with("\"sha256:")),
        "{r:?}"
    );
    let about = get(&server, &testing_hub::IDENTITY.route("about"), Some(&token)).await;
    assert!(about.json()["engine"]["version"].is_string(), "{about:?}");
}

/// A real run of testing.nvim in a repository of the fixture, into the fixture's state folder: what
/// a person does in the terminal while the page is open. Its one case fails.
fn a_red_run(fx: &Fixture, repo: &str, nvim: &Path, script: &Path) {
    let dir = fx.root().join(repo);
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(
        dir.join("tests").join("red_spec.lua"),
        "describe('red', function()\n  it('fails', function()\n    assert.is_true(false)\n  end)\nend)\n",
    )
    .unwrap();
    let status = Command::new(nvim)
        .args(["-n", "-i", "NONE", "--headless", "-u", "NONE", "-l"])
        .arg(script)
        .args(["run", ".", "--state-dir"])
        .arg(fx.dir.join("state"))
        .current_dir(&dir)
        // `CI` trusts the root in the temp dir.
        .env("CI", "true")
        .status()
        .expect("nvim runs");
    assert_eq!(status.code(), Some(1), "a red run ends with exit 1");
}

#[tokio::test]
async fn a_real_run_is_seen_by_the_watcher_and_its_findings_are_one_document() {
    let fx = Fixture::new();
    let env = Env::from_process();
    let file = FileConfig::<TestingSettings> {
        vault: Some(fx.root()),
        ..FileConfig::default()
    };
    if engine_or_skip(&fx.root()).await.is_none() {
        return;
    }
    let setup = discover(&DISCOVERY, &file, AppInputs::default(), &env)
        .await
        .expect("the engine was found a moment ago");
    let config = cli_config(&setup, env);
    let (nvim, script) = (config.nvim.clone(), config.script.clone());
    let engine = CliEngine::new(config);
    let info = probe(&engine).await.expect("hello");
    let control = Secret::generate(TOKEN_BYTES).unwrap();
    let server = testing_hub::start(testing_hub::Start {
        addr: "127.0.0.1:0".parse().unwrap(),
        limits: Limits {
            snapshot_max_age: None,
            ..Limits::default()
        },
        clock: Arc::new(ManualClock::new()),
        pairing_ttl: Duration::from_secs(60),
        control_secret: control.clone(),
        instance: Secret::generate(CODE_BYTES).unwrap(),
        status: appkit_hub::HubStatus {
            engine_ready: true,
            problem: None,
            capabilities: info.capabilities,
        },
        engine: Some(Arc::new(engine) as Arc<dyn Engine>),
        ask: fx.ask(),
        origins: Origins::default(),
        watch: Some(Watch {
            state_dir: fx.dir.join("state"),
            refresh: RefreshOptions::default(),
        }),
        ui: testing_hub::ui::UiFiles::none(),
    })
    .await
    .expect("the hub starts");
    let token = session(&server, &control).await;
    let fleet = || async { get(&server, &route_of("fleet"), Some(&token)).await };

    let before = fleet().await.json();
    let row = |doc: &Value| {
        doc["repos"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == "alpha.nvim")
            .cloned()
            .unwrap()
    };
    let key = row(&before)["key"].as_str().unwrap().to_owned();
    assert!(row(&before)["local"].is_null(), "no run yet: {before}");

    // With no run, no finding is an empty list in one document.
    let none = get(
        &server,
        &format!("{}?key={key}", route_of("finding")),
        Some(&token),
    )
    .await;
    assert_eq!(none.status, 200, "{none:?}");
    assert_eq!(none.json()["kind"], "findings");
    assert_eq!(none.json()["findings"], serde_json::json!([]));

    let mut stream = Stream::open(&server, &route_of("events"), &token).await;
    stream.until("event: hello").await;
    // Kept by the task and given back: the hub reads below this folder until the end of the test.
    let fx = tokio::task::spawn_blocking(move || {
        a_red_run(&fx, "alpha.nvim", &nvim, &script);
        fx
    })
    .await
    .unwrap();

    // Nothing asks: the stream says the fleet changed, and the next read shows the red run.
    stream
        .until_within("event: fleet", Duration::from_secs(30))
        .await;
    let after = fleet().await.json();
    assert_ne!(after["digest"], before["digest"]);
    assert_eq!(row(&after)["local"]["verdict"], "red", "{after}");

    let found = get(
        &server,
        &format!("{}?key={key}", route_of("finding")),
        Some(&token),
    )
    .await;
    assert_eq!(found.status, 200, "{found:?}");
    let list = found.json();
    let list = list["findings"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["schema"], "testing-finding/1");
    assert_eq!(list[0]["kind"], "case");
    drop(fx);
}

fn route_of(name: &str) -> String {
    testing_hub::IDENTITY.route(name)
}
