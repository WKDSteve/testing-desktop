//! testing mobil on the hub: the security matrix with phones, and what a phone may do here. It reads the fleet and
//! starts the failed jobs of a red run again, with the level `trigger` and a confirmation of that very request. It
//! never steps a sheet, opens the event stream, or reaches anything that trusts or releases a commit (decided
//! 2026-11-11: never from the phone).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use appkit_core::authn::{ManualClock, Secret, CODE_BYTES, TOKEN_BYTES};
use appkit_core::identity::AppIdentity;
use appkit_hub::{BoxFuture, HubStatus, Limits, RunningServer};
use appkit_testkit::subject::{StartConfig, Subject};
use p256::ecdsa::signature::Signer as _;
use serde_json::{json, Value};
use testing_hub::engine::{CallFuture, Engine, Method, Reply};

mod common;

use common::{get, post, session};

const FLEET: &str = include_str!("fixtures/golden/fleet.json");
const HELLO: &str = include_str!("fixtures/golden/hello.json");
const TAILNET: &str = "pc.example.ts.net";

/// An engine that knows the fleet only.
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

/// A rerun that is kept, not done.
#[derive(Default)]
struct Reruns(Mutex<Vec<(String, u64)>>);

impl testing_hub::rerun::Rerunner for Reruns {
    fn rerun(&self, repo: &str, id: u64) -> BoxFuture<'_, Result<(), String>> {
        self.0.lock().unwrap().push((repo.to_owned(), id));
        Box::pin(async { Ok(()) })
    }
}

/// The testing hub with phones: devices in a scratch folder, the tailnet name, push into a list.
struct TestingPhone {
    dir: appkit_scratch::Scratch,
    push: Arc<appkit_hub::push::Push>,
    engine: bool,
    reruns: Arc<Reruns>,
}

impl TestingPhone {
    fn new(engine: bool) -> Self {
        let dir = appkit_scratch::Scratch::new("testing-phone");
        let sent = Arc::new(appkit_testkit::subjects::SentPushes::default());
        let push = Arc::new(appkit_hub::push::Push::open(&dir, sent).unwrap());
        Self {
            dir,
            push,
            engine,
            reruns: Arc::default(),
        }
    }
}

impl Subject for TestingPhone {
    fn identity(&self) -> &'static AppIdentity {
        &testing_hub::IDENTITY
    }

    fn start(&self, cfg: StartConfig) -> BoxFuture<'_, std::io::Result<RunningServer>> {
        let engine = self
            .engine
            .then(|| Arc::new(FleetEngine) as Arc<dyn Engine>);
        let status = if self.engine {
            HubStatus {
                engine_ready: true,
                problem: None,
                capabilities: Vec::new(),
            }
        } else {
            cfg.status
        };
        Box::pin(testing_hub::start(testing_hub::Start {
            addr: cfg.addr,
            limits: cfg.limits,
            clock: cfg.clock,
            pairing_ttl: cfg.pairing_ttl,
            control_secret: cfg.control_secret,
            instance: cfg.instance,
            status,
            engine,
            ask: testing_hub::engine::Ask::default(),
            origins: testing_hub::Origins::default(),
            watch: None,
            step: None,
            tasks_vault: None,
            ui: testing_hub::ui::UiFiles::none(),
            warm: false,
            phone: Some(testing_hub::phone::Phone {
                devices_file: self.dir.join("devices.json"),
                tailnet: Some(TAILNET.to_owned()),
                push: Some(Arc::clone(&self.push)),
            }),
            rerun: Some(Arc::clone(&self.reruns) as Arc<dyn testing_hub::rerun::Rerunner>),
        }))
    }

    fn data_routes(&self) -> Vec<&'static str> {
        testing_hub::DATA_ROUTES.to_vec()
    }
}

mod testing_phone {
    appkit_testkit::security_matrix!(crate::TestingPhone::new(false));
}

#[tokio::test]
async fn a_phone_pairs_once_and_the_pc_revokes_it() {
    appkit_testkit::security::a_device_pairs_once_and_then_logs_in_with_its_key(
        &TestingPhone::new(false),
    )
    .await;
    appkit_testkit::security::the_pc_revokes_a_device_and_its_sessions_end(&TestingPhone::new(
        false,
    ))
    .await;
}

#[tokio::test]
async fn through_the_tailnet_only_a_phone_gets_in() {
    appkit_testkit::security::the_tailnet_takes_devices_only(&TestingPhone::new(false), TAILNET)
        .await;
}

#[tokio::test]
async fn a_phone_subscribes_to_push_and_the_pc_cannot() {
    let subject = TestingPhone::new(false);
    let push = Arc::clone(&subject.push);
    appkit_testkit::security::devices_subscribe_to_push_and_nothing_else_can(&subject, &|id| {
        !push.kinds_of(id).is_empty()
    })
    .await;
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn public(key: &p256::ecdsa::SigningKey) -> String {
    hex(key.verifying_key().to_sec1_point(false).as_bytes())
}

fn sign(key: &p256::ecdsa::SigningKey, text: &str) -> String {
    let sig: p256::ecdsa::Signature = key.sign(text.as_bytes());
    hex(&sig.to_bytes())
}

async fn start(subject: &TestingPhone) -> (RunningServer, Secret) {
    let control = Secret::generate(TOKEN_BYTES).unwrap();
    let server = subject
        .start(StartConfig {
            addr: "127.0.0.1:0".parse().unwrap(),
            limits: Limits::default(),
            clock: Arc::new(ManualClock::new()),
            pairing_ttl: Duration::from_secs(60),
            control_secret: control.clone(),
            instance: Secret::generate(CODE_BYTES).unwrap(),
            status: testing_hub::app::no_engine_status(),
        })
        .await
        .unwrap();
    (server, control)
}

/// A request with the extra headers given.
async fn send_with(
    server: &RunningServer,
    method: &str,
    path: &str,
    token: &str,
    extra: &[(&str, &str)],
    body: &str,
) -> appkit_testkit::wire::Reply {
    let host = format!("127.0.0.1:{}", server.addr().port());
    let origin = format!("http://{host}");
    let bearer = format!("Bearer {token}");
    let len = body.len().to_string();
    let mut headers = vec![
        ("Host", host.as_str()),
        ("Origin", origin.as_str()),
        ("Authorization", bearer.as_str()),
    ];
    if method == "POST" {
        headers.push(("Content-Type", "application/json"));
        headers.push(("Content-Length", len.as_str()));
    }
    headers.extend_from_slice(extra);
    common::send(
        server.addr(),
        appkit_testkit::wire::request(method, path, &headers, body.as_bytes()),
    )
    .await
}

#[tokio::test]
async fn a_phone_reads_reruns_and_runs_a_tasks_sheets_only_confirmed_and_never_steps_a_sheet() {
    let subject = TestingPhone::new(true);
    let (server, control) = start(&subject).await;
    let origin = format!("http://127.0.0.1:{}", server.addr().port());
    let route = |n: &str| testing_hub::IDENTITY.route(n);
    // Pair a phone with its login key and its confirmation key.
    let login = p256::ecdsa::SigningKey::from_slice(&[7; 32]).unwrap();
    let confirm = p256::ecdsa::SigningKey::from_slice(&[8; 32]).unwrap();
    let code = post(&server, "/api/control/pair", Some(&control.to_hex()), "{}").await;
    let code = code.json()["code"].as_str().unwrap().to_owned();
    let body = json!({ "code": code, "name": "Fairphone", "key": public(&login), "confirm_key": public(&confirm) })
        .to_string();
    let r = post(&server, "/auth/device/pair", None, &body).await;
    assert_eq!(r.status, 200, "{r:?}");
    let id = r.json()["device"].as_str().unwrap().to_owned();
    let token = r.json()["token"].as_str().unwrap().to_owned();

    // Reads.
    for name in ["fleet", "about"] {
        let r = get(&server, &route(name), Some(&token)).await;
        assert_eq!(r.status, 200, "{name}: {r:?}");
    }
    // Never: a stepped run of a sheet (it runs code), its commands, the event stream.
    for (method, name) in [
        ("POST", "sheet-step/start"),
        ("POST", "sheet-step/command"),
        ("GET", "sheet-step"),
        ("GET", "sheet-step/sheets"),
        ("GET", "events"),
        // Nor the run of a whole suite or of a file: a new run from the phone is only that of a task's sheets.
        ("POST", "run/start"),
        // Never the release of a commit for the runner, nor a step of its assistant.
        ("POST", "release"),
        ("GET", "release/repos"),
        ("GET", "release/commits"),
        ("GET", "release/show"),
        ("GET", "release/outcome"),
    ] {
        let r = send_with(
            &server,
            method,
            &route(name),
            &token,
            &[],
            r#"{"key":"x","sheet":"y"}"#,
        )
        .await;
        assert_eq!(r.status, 403, "{method} {name}: {r:?}");
        assert_eq!(r.json()["error"]["code"], "not_for_devices", "{name}");
    }
    // Where a run stands, cut for a device; there is none yet.
    let r = get(&server, &route("run"), Some(&token)).await;
    assert_eq!(r.status, 200, "{r:?}");
    assert_eq!(r.json()["run"], Value::Null, "{r:?}");
    // A new phone may not start anything.
    let task = r#"{"task":"alpha.nvim/try-it"}"#;
    let r = send_with(&server, "POST", &route("run/task"), &token, &[], task).await;
    assert_eq!(r.json()["error"]["code"], "level_too_low", "{r:?}");
    let red = r#"{"key":"beta.nvim-ac6d3a4ece7f"}"#;
    let r = send_with(&server, "POST", &route("ci-rerun"), &token, &[], red).await;
    assert_eq!(r.json()["error"]["code"], "level_too_low", "{r:?}");
    // The PC grants it; still nothing without the confirmation of that very request.
    let r = post(
        &server,
        "/api/control/devices/level",
        Some(&control.to_hex()),
        &json!({ "device": id, "level": "trigger" }).to_string(),
    )
    .await;
    assert_eq!(r.status, 200, "{r:?}");
    let r = send_with(&server, "POST", &route("ci-rerun"), &token, &[], red).await;
    assert_eq!(r.json()["error"]["code"], "confirm_required", "{r:?}");
    assert!(subject.reruns.0.lock().unwrap().is_empty());
    let r = send_with(&server, "POST", &route("run/task"), &token, &[], task).await;
    assert_eq!(r.json()["error"]["code"], "confirm_required", "{r:?}");
    // Confirmed: the failed jobs of run 22 of fixture-owner/beta.nvim are started again.
    let confirmed_for = |name: &'static str, body: &'static str| {
        let origin = origin.clone();
        let id = id.clone();
        let path = route(name);
        let server = &server;
        let confirm = &confirm;
        async move {
            let c = post(
                server,
                "/auth/device/challenge",
                None,
                &json!({ "device": id }).to_string(),
            )
            .await;
            let c = c.json()["challenge"].as_str().unwrap().to_owned();
            let digest = {
                use sha2::Digest;
                hex(&sha2::Sha256::digest(body.as_bytes()))
            };
            let text =
                appkit_hub::devices::confirm_message(&origin, &id, &c, "POST", &path, &digest);
            format!("{c}.{}", sign(confirm, &text))
        }
    };
    let confirmed = |body: &'static str| confirmed_for("ci-rerun", body);
    // Confirmed, the run of a task's sheets passes the guard and reaches the hub (which has no engine to run here).
    let header = confirmed_for("run/task", task).await;
    let r = send_with(
        &server,
        "POST",
        &route("run/task"),
        &token,
        &[("x-device-confirm", &header)],
        task,
    )
    .await;
    assert_eq!(r.json()["error"]["code"], "run_unavailable", "{r:?}");
    // A confirmation is for that very request: one made for the rerun does not start a run.
    let header = confirmed(red).await;
    let r = send_with(
        &server,
        "POST",
        &route("run/task"),
        &token,
        &[("x-device-confirm", &header)],
        task,
    )
    .await;
    assert_eq!(r.json()["error"]["code"], "confirm_required", "{r:?}");
    let header = confirmed(red).await;
    let r = send_with(
        &server,
        "POST",
        &route("ci-rerun"),
        &token,
        &[("x-device-confirm", &header)],
        red,
    )
    .await;
    assert_eq!(r.status, 202, "{r:?}");
    assert_eq!(
        *subject.reruns.0.lock().unwrap(),
        [("fixture-owner/beta.nvim".to_owned(), 22)]
    );
    // A green one is not started, confirmed or not.
    let green = r#"{"key":"alpha.nvim-356bf237c7fc"}"#;
    let header = confirmed(green).await;
    let r = send_with(
        &server,
        "POST",
        &route("ci-rerun"),
        &token,
        &[("x-device-confirm", &header)],
        green,
    )
    .await;
    assert_eq!(r.status, 409, "{r:?}");
    assert_eq!(subject.reruns.0.lock().unwrap().len(), 1);
    // Anything but a key is refused before anything is looked up.
    let r = send_with(
        &server,
        "POST",
        &route("ci-rerun"),
        &session(&server, &control).await,
        &[],
        r#"{"key":"beta.nvim-ac6d3a4ece7f","command":"testing ci trust"}"#,
    )
    .await;
    assert_eq!(r.status, 400, "{r:?}");
    assert_eq!(subject.reruns.0.lock().unwrap().len(), 1);
}
