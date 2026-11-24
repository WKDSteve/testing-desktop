//! The switch of the page for the start with the login (`GET`/`POST /api/testing/autostart`), on the kit's one state
//! (`appkit_hub::loginstate`) with a login of the test and a settings folder of hub-desktop of the test: what the page
//! switches is what hub-desktop reads (`start_with_hub`), and what hub-desktop switched is what the page shows. Never the
//! login of the machine.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use appkit_core::authn::{ManualClock, Secret, CODE_BYTES, TOKEN_BYTES};
use appkit_core::identity::AppIdentity;
use appkit_hub::env::Env;
use appkit_hub::loginstate::{self, LoginEntries, HUB_DESKTOP};
use appkit_hub::{Limits, RunningServer};
use appkit_scratch::Scratch;
use testing_hub::engine::Ask;
use testing_hub::loginentry::LoginEntry;
use testing_hub::Origins;

mod common;

use common::{get, post, session};

/// The login entries of a machine made for the test: the apps that have one.
#[derive(Clone, Default)]
struct FakeLogin(Arc<Mutex<BTreeSet<String>>>);

impl FakeLogin {
    fn has(&self, app: &str) -> bool {
        self.0.lock().unwrap().contains(app)
    }
}

impl LoginEntries for FakeLogin {
    fn is_on(&self, identity: &AppIdentity) -> Option<bool> {
        Some(self.has(identity.app))
    }
    fn set(&self, identity: &AppIdentity, program: Option<&Path>) -> Result<(), String> {
        let mut held = self.0.lock().unwrap();
        if program.is_some() {
            held.insert(identity.app.to_owned());
        } else {
            held.remove(identity.app);
        }
        Ok(())
    }
}

/// The page's switch on the kit's state, as `SystemLoginEntry` is, with the login and the folder of the test.
struct KitLogin {
    login: FakeLogin,
    env: Env,
}

impl LoginEntry for KitLogin {
    fn is_on(&self) -> Option<bool> {
        loginstate::is_on(&testing_hub::IDENTITY, &self.env, &self.login)
    }
    fn set(&self, on: bool) -> Result<(), String> {
        loginstate::set(
            &testing_hub::IDENTITY,
            on,
            || Ok(PathBuf::from("C:/apps/testing-hub.exe")),
            &self.env,
            &self.login,
        )
        .map(|_| ())
    }
}

async fn start(login: Option<Arc<dyn LoginEntry>>) -> (RunningServer, String) {
    let control = Secret::generate(TOKEN_BYTES).unwrap();
    let server = testing_hub::start(testing_hub::Start {
        addr: "127.0.0.1:0".parse().unwrap(),
        limits: Limits::default(),
        clock: Arc::new(ManualClock::new()),
        pairing_ttl: Duration::from_secs(60),
        control_secret: control.clone(),
        instance: Secret::generate(CODE_BYTES).unwrap(),
        status: testing_hub::app::no_engine_status(),
        engine: None,
        ask: Ask::default(),
        origins: Origins::default(),
        watch: None,
        step: None,
        tasks_vault: None,
        ui: testing_hub::ui::UiFiles::none(),
        warm: false,
        phone: None,
        rerun: None,
        login,
    })
    .await
    .expect("the hub starts");
    let token = session(&server, &control).await;
    (server, token)
}

fn route() -> String {
    testing_hub::IDENTITY.route("autostart")
}

#[tokio::test]
async fn the_page_and_hub_desktop_switch_one_state_and_show_what_the_other_did() {
    let folder = Scratch::new("testing-autostart");
    let env = Env::from_pairs([(
        "HUB_DESKTOP_CONFIG_DIR",
        folder.path().as_os_str().to_owned(),
    )]);
    let login = FakeLogin::default();
    // hub-desktop starts with the login.
    login
        .set(&HUB_DESKTOP, Some(Path::new("C:/apps/hub-desktop.exe")))
        .unwrap();
    let page = KitLogin {
        login: login.clone(),
        env: env.clone(),
    };
    let (server, token) = start(Some(Arc::new(page))).await;
    let shown = || async { get(&server, &route(), Some(&token)).await.json()["on"].clone() };
    assert_eq!(shown().await, false);

    // Switched on in the page: hub-desktop lists it, and its tile reads it as on.
    let on = post(&server, &route(), Some(&token), r#"{"on":true}"#).await;
    assert_eq!(on.status, 200, "{on:?}");
    assert_eq!(on.json()["on"], true);
    assert_eq!(loginstate::read_list(folder.path()).unwrap(), ["testing"]);
    assert!(!login.has("testing-desktop"));

    // Switched off in the tile of hub-desktop (the same kit call): the page shows it.
    loginstate::set(
        &testing_hub::IDENTITY,
        false,
        || Err("unused".into()),
        &env,
        &login,
    )
    .unwrap();
    assert_eq!(shown().await, false);

    // hub-desktop does not start with the login: the switch writes the hub's own entry, shown all the same.
    login.set(&HUB_DESKTOP, None).unwrap();
    let own = post(&server, &route(), Some(&token), r#"{"on":true}"#).await;
    assert_eq!(own.json()["on"], true, "{own:?}");
    assert!(login.has("testing-desktop"));
    assert!(loginstate::read_list(folder.path()).unwrap().is_empty());
}

#[tokio::test]
async fn without_a_login_entry_the_page_gets_no_switch_and_a_body_must_be_one() {
    let (server, token) = start(None).await;
    assert!(get(&server, &route(), Some(&token)).await.json()["on"].is_null());
    let refused = post(&server, &route(), Some(&token), r#"{"on":true}"#).await;
    assert_eq!(refused.status, 404, "{refused:?}");

    let (server, token) = start(Some(Arc::new(KitLogin {
        login: FakeLogin::default(),
        env: Env::from_pairs([("HUB_DESKTOP_CONFIG_DIR", "C:/nowhere-of-the-test")]),
    })))
    .await;
    for body in ["{}", r#"{"on":"yes"}"#, r#"{"on":true,"more":1}"#, "nope"] {
        let reply = post(&server, &route(), Some(&token), body).await;
        assert_eq!(reply.status, 400, "{body}: {reply:?}");
    }
}

/// A login entry whose switch panics: the route cannot say why, so it answers a 500 with a code and a fixed text.
struct PanickingLogin;

impl LoginEntry for PanickingLogin {
    fn is_on(&self) -> Option<bool> {
        Some(false)
    }
    fn set(&self, _on: bool) -> Result<(), String> {
        panic!("C:/Users/someone/secret/login-entry");
    }
}

#[tokio::test]
async fn a_500_of_the_hub_has_a_code_and_a_fixed_text_and_no_path() {
    let (server, token) = start(Some(Arc::new(PanickingLogin))).await;
    let r = post(&server, &route(), Some(&token), r#"{"on":true}"#).await;
    assert_eq!(r.status, 500, "{r:?}");
    assert_eq!(r.json()["error"]["code"], "autostart_failed");
    let raw = r.raw.to_lowercase();
    assert!(!raw.contains("someone"), "{raw}");
    assert!(!raw.contains("c:/"), "{raw}");
    assert!(!raw.contains("set-cookie"), "{raw}");
}
