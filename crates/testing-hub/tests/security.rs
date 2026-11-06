//! The security matrix of appkit (guard, pairing, limits), run for the testing hub, and what the hub
//! answers of its own. It runs before the first route of data exists, and every route added later is
//! tried by it through `DATA_ROUTES`.

use std::sync::Arc;
use std::time::Duration;

use appkit_core::authn::{ManualClock, Secret, CODE_BYTES, TOKEN_BYTES};
use appkit_core::identity::AppIdentity;
use appkit_hub::{BoxFuture, Limits, RunningServer};
use appkit_testkit::subject::{StartConfig, Subject};
use appkit_testkit::subjects::DEMO_IDENTITY;

mod common;

use common::{get, session};

/// The testing hub, as the matrix starts it.
pub struct Testing;

impl Subject for Testing {
    fn identity(&self) -> &'static AppIdentity {
        &testing_hub::IDENTITY
    }

    fn start(&self, cfg: StartConfig) -> BoxFuture<'_, std::io::Result<RunningServer>> {
        Box::pin(testing_hub::start(testing_hub::Start {
            addr: cfg.addr,
            limits: cfg.limits,
            clock: cfg.clock,
            pairing_ttl: cfg.pairing_ttl,
            control_secret: cfg.control_secret,
            instance: cfg.instance,
            status: cfg.status,
            engine: None,
            ask: testing_hub::engine::Ask::default(),
            origins: testing_hub::Origins::default(),
            watch: None,
            step: None,
            ui: testing_hub::ui::UiFiles::none(),
        }))
    }

    fn data_routes(&self) -> Vec<&'static str> {
        testing_hub::DATA_ROUTES.to_vec()
    }
}

mod testing {
    appkit_testkit::security_matrix!(crate::Testing);
}

#[tokio::test]
async fn about_names_the_app_to_a_session_and_to_nobody_else() {
    let control = Secret::generate(TOKEN_BYTES).unwrap();
    let server = Testing
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
        .expect("the hub starts");

    let hello = get(&server, "/api/testing/hello", None).await;
    assert_eq!(hello.status, 200, "{hello:?}");
    assert_eq!(hello.json()["app"], "testing-desktop");

    let about = testing_hub::IDENTITY.route("about");
    assert_eq!(get(&server, &about, None).await.status, 401);

    let token = session(&server, &control).await;

    let r = get(&server, &about, Some(&token)).await;
    assert_eq!(r.status, 200, "{r:?}");
    assert_eq!(r.json()["app"], "testing-desktop");
    assert_eq!(r.json()["version"], testing_hub::version());
    // No engine: the app is still described, and nothing claims an engine.
    assert!(r.json()["engine"].is_null(), "{r:?}");

    // A query is refused, not ignored.
    let q = get(&server, &format!("{about}?x=1"), Some(&token)).await;
    assert_eq!(q.status, 400, "{q:?}");
}

#[tokio::test]
async fn hello_says_the_engine_is_not_ready_and_why() {
    let server = Testing
        .start(StartConfig {
            addr: "127.0.0.1:0".parse().unwrap(),
            limits: Limits::default(),
            clock: Arc::new(ManualClock::new()),
            pairing_ttl: Duration::from_secs(60),
            control_secret: Secret::generate(TOKEN_BYTES).unwrap(),
            instance: Secret::generate(CODE_BYTES).unwrap(),
            status: testing_hub::app::no_engine_status(),
        })
        .await
        .expect("the hub starts");
    let hello = get(&server, "/api/testing/hello", None).await;
    assert_eq!(hello.status, 200, "{hello:?}");
    let doc = hello.json();
    assert_eq!(doc["engine"]["ready"], false, "{doc}");
    assert_eq!(
        doc["engine"]["problem"], "no_engine",
        "hello names why: {doc}"
    );
}

/// The names of the other hubs, copied: they are no dependency here, and the hubs run on one machine
/// side by side.
const TASKS_IDENTITY: AppIdentity = AppIdentity {
    app: "tasks-desktop",
    binary: "tasks-hub",
    env_prefix: "TASKS_DESKTOP",
    title: "Tasks",
    default_port: 17615,
    route_ns: "tasks",
};

const RULES_IDENTITY: AppIdentity = AppIdentity {
    app: "rules-desktop",
    binary: "rules-hub",
    env_prefix: "RULES_DESKTOP",
    title: "Rules",
    default_port: 17616,
    route_ns: "rules",
};

const HUB_IDENTITY: AppIdentity = AppIdentity {
    app: "hub-desktop",
    binary: "hub-desktop",
    env_prefix: "HUB_DESKTOP",
    title: "Hub",
    default_port: 17610,
    route_ns: "hub",
};

/// Every name that lands on the disk, in the registry or in the environment of a person.
fn names(id: &AppIdentity) -> Vec<(&'static str, String)> {
    vec![
        ("app", id.app.to_owned()),
        ("program file", id.program_file()),
        ("config dir variable", id.var("CONFIG_DIR")),
        ("install dir variable", id.var("INSTALL_DIR")),
        ("port variable", id.var("PORT")),
        ("no-login-sync variable", id.var("NO_LOGIN_SYNC")),
        ("desktop file", id.desktop_file()),
        ("agent file", id.agent_file()),
        ("agent label", id.agent_label()),
        ("route of hello", id.route("hello")),
        ("default port", id.default_port.to_string()),
    ]
}

/// The name of the first name two identities share, if any.
fn shared_name(a: &AppIdentity, b: &AppIdentity) -> Option<String> {
    names(a)
        .into_iter()
        .zip(names(b))
        .find(|((_, x), (_, y))| x == y)
        .map(|((what, x), _)| format!("{what}: {x}"))
}

#[test]
fn no_name_of_the_testing_hub_is_a_name_of_another_app() {
    // The app's config folder, install folder and login entry, its variables and its port: a hub
    // that shared one of them with another would write over that one's files or settings.
    for (other, theirs) in [
        ("the demo app", &DEMO_IDENTITY),
        ("tasks-hub", &TASKS_IDENTITY),
        ("rules-hub", &RULES_IDENTITY),
        ("hub-desktop", &HUB_IDENTITY),
    ] {
        assert_eq!(
            shared_name(&testing_hub::IDENTITY, theirs),
            None,
            "a name the same as {other}'s"
        );
    }
}

#[test]
fn a_copy_of_the_names_of_rules_hub_is_caught() {
    // The likely slip: the identity of the template taken over with only the port changed.
    let copied = AppIdentity {
        default_port: 17699,
        ..RULES_IDENTITY
    };
    assert_eq!(
        shared_name(&copied, &RULES_IDENTITY),
        Some("app: rules-desktop".to_owned())
    );
    assert_eq!(shared_name(&testing_hub::IDENTITY, &copied), None);
}

/// The port is the one the list of appkit gives this app.
#[test]
fn the_port_is_the_one_appkit_lists_for_testing_desktop() {
    let entry = appkit_testkit::ports::APP_PORTS
        .iter()
        .find(|p| p.port == testing_hub::IDENTITY.default_port)
        .expect("the port of the testing hub is in appkit's list");
    assert!(entry.owner.starts_with("testing-desktop"), "{entry:?}");
}
