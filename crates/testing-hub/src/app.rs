//! The commands of the program: `serve` and `open`, and the launcher of the browser.
//!
//! They live in the library so that their parts can be tested; `main.rs` only reads the arguments.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use appkit_core::authn::{Secret, SystemClock, CODE_BYTES, TOKEN_BYTES};
use appkit_core::registry::{self, HubRecord};
use appkit_hub::config::FileConfig;
use appkit_hub::discovery::{discover, AppInputs};
use appkit_hub::env::Env;
use appkit_hub::{HubStatus, Limits};
use serde_json::Value;

use crate::engine::{cli_config, probe, Ask, CliEngine, Engine, DISCOVERY};
use crate::github::CiState;
use crate::settings::TestingSettings;
use crate::sheetstep::StepProgram;
use crate::store::RefreshOptions;
use crate::ui::UiFiles;
use crate::{client, Origins, Start, Watch, IDENTITY};

/// What to do when the hub is started.
#[derive(Debug, Clone, Default)]
pub struct ServeOptions {
    /// Overrides the settings file and the environment.
    pub port: Option<u16>,
}

/// The folder for `hub.json`, `control.secret` and `config.json`.
fn app_dir(env: &Env) -> Result<PathBuf, String> {
    appkit_hub::instance::config_folder(env, &IDENTITY)
}

/// `testing-hub phone ...`: the phones of the running hub (testing mobil), with its control secret.
///
/// # Errors
///
/// A text for the person: the usage, no running hub, or the hub's refusal.
pub async fn phone(args: &[String], usage: &str) -> Result<String, String> {
    use appkit_hub::phone::{self, PhoneWay};
    let env = Env::from_process();
    let dir = app_dir(&env)?;
    match args {
        [cmd, rest @ ..] if cmd == "pair" => {
            let way = match rest {
                [] => {
                    // The tailnet name of the settings, else the tunnel.
                    let file: FileConfig<TestingSettings> =
                        appkit_hub::config::load_file(&dir).map_err(|e| e.to_string())?;
                    file.app
                        .tailnet_name
                        .map_or(PhoneWay::Tunnel, PhoneWay::Tailnet)
                }
                [flag] if flag == "--tunnel" => PhoneWay::Tunnel,
                [flag, name] if flag == "--tailnet" && crate::phone::plain_tailnet_name(name) => {
                    PhoneWay::Tailnet(name.clone())
                }
                _ => return Err(usage.to_owned()),
            };
            let link = phone::pairing_link(&dir, &IDENTITY, &way).await?;
            let qr = phone::qr_text(&link)?;
            Ok(format!(
                "{qr}\n{link}\n\nScan the code with testing mobil (or paste the link) within 60 seconds.\n"
            ))
        }
        [cmd] if cmd == "list" => phone::device_lines(&dir, &IDENTITY).await,
        [cmd, id, level] if cmd == "level" => phone::set_level(&dir, &IDENTITY, id, level)
            .await
            .map(|()| format!("{id}: level {level}\n")),
        [cmd, id] if cmd == "revoke" => phone::revoke(&dir, &IDENTITY, id)
            .await
            .map(|()| format!("{id}: revoked; its sessions ended\n")),
        _ => Err(usage.to_owned()),
    }
}

/// The port to try first: the option, the environment, the settings file, then the default.
fn wanted_port(
    opts: &ServeOptions,
    file: &FileConfig<TestingSettings>,
    env: &Env,
) -> Result<u16, String> {
    if let Some(p) = opts.port {
        return Ok(p);
    }
    let port_var = IDENTITY.var("PORT");
    if let Some(v) = env.get(&port_var) {
        let text = v.to_string_lossy();
        return text
            .parse::<u16>()
            .ok()
            .filter(|p| *p >= 1024)
            .ok_or_else(|| format!("{port_var} is not a port from 1024 to 65535: {text:?}"));
    }
    match file.port {
        Some(p) if p < 1024 => Err(format!(
            "port in config.json is not a port from 1024 to 65535: {p}"
        )),
        Some(p) => Ok(p),
        None => Ok(IDENTITY.default_port),
    }
}

/// Is a hub of this app running according to `hub.json`? It is only believed when the process on
/// that port says the same instance.
async fn running_hub(dir: &std::path::Path) -> Option<HubRecord> {
    appkit_hub::instance::running_hub(dir, &IDENTITY).await
}

/// Wait for Ctrl-C, or for SIGTERM where there is one.
async fn wait_for_stop() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

/// What `hello` says when there is no engine: not ready, and why, as a short code.
#[must_use]
pub fn no_engine_status() -> HubStatus {
    HubStatus {
        engine_ready: false,
        problem: Some("no_engine"),
        capabilities: Vec::new(),
    }
}

/// What the start found out about the engine.
pub struct EngineState {
    pub status: HubStatus,
    /// Only when the engine answered `hello` as this app needs.
    pub engine: Option<Arc<dyn Engine>>,
    /// How a stepped run of a sheet is started with that engine; with `engine`.
    pub step: Option<StepProgram>,
    /// A longer text for the terminal of whoever started the hub (it may hold paths; `hello` gets
    /// only the short code of `status`).
    pub note: Option<String>,
}

/// The settings the search for the engine reads, with the repositories root defaulted: without
/// `vault` in `config.json` and without `TESTING_REPOS_ROOT`, the root of all repositories
/// (`REPOS_DIR`), where testing.nvim is too.
#[must_use]
pub fn with_root_default(
    file: &FileConfig<TestingSettings>,
    env: &Env,
) -> FileConfig<TestingSettings> {
    let mut file = file.effective();
    if file.vault.is_none() && env.path(DISCOVERY.vault_var).is_none() {
        file.vault = env.path("REPOS_DIR");
    }
    file
}

/// Find Neovim, testing.nvim and the repositories root, start the engine and ask it `hello`.
pub async fn engine_state(file: &FileConfig<TestingSettings>, env: &Env) -> EngineState {
    let failed = |code: &'static str, note: String| EngineState {
        status: HubStatus {
            engine_ready: false,
            problem: Some(code),
            capabilities: Vec::new(),
        },
        engine: None,
        step: None,
        note: Some(note),
    };
    let file = with_root_default(file, env);
    let setup = match discover(&DISCOVERY, &file, AppInputs::default(), env).await {
        Ok(setup) => setup,
        Err(problem) => return failed(problem.code(), problem.to_string()),
    };
    // One process per question: testing.nvim has no persistent host.
    let engine = Arc::new(CliEngine::new(cli_config(&setup, env.clone())));
    match probe(engine.as_ref()).await {
        Ok(info) => EngineState {
            status: HubStatus {
                engine_ready: true,
                problem: None,
                capabilities: info.capabilities,
            },
            engine: Some(engine as Arc<dyn Engine>),
            step: Some(StepProgram::from_setup(&setup, env)),
            note: setup
                .lib_problem
                .as_ref()
                .map(|p| format!("note: {p} (the engine may still find it)")),
        },
        Err(problem) => failed(problem.code(), problem.to_string()),
    }
}

/// How long a snapshot is good for while the state folder is watched: a change renews it at once, so
/// this is only the net under a lost event for a page that keeps no stream open. A read of a fleet
/// starts a Neovim process and asks git about every repository. Without a watcher the default of the
/// kit holds (a page must not see old results for long).
const WATCHED_SNAPSHOT_MAX_AGE: Duration = Duration::from_secs(45);

/// What a request may take beyond the call of the engine it waits for: the hub's own work around it.
const REQUEST_MARGIN: Duration = Duration::from_secs(15);

/// The limits of the hub, with the age of the snapshot that fits how it learns of a change.
///
/// A request that finds no snapshot waits for the engine to read the fleet, which takes most of a minute with GitHub
/// (and up to `ENGINE_TIMEOUT`). The limit of the kit (30 s) is made for an engine that answers in a moment: with it
/// the first page of a fleet that asks GitHub got "the request took too long" while the engine was still reading.
fn limits_for(watching: bool) -> Limits {
    let limits = Limits {
        request_timeout: crate::engine::ENGINE_TIMEOUT + REQUEST_MARGIN,
        ..Limits::default()
    };
    if watching {
        Limits {
            snapshot_max_age: Some(WATCHED_SNAPSHOT_MAX_AGE),
            ..limits
        }
    } else {
        limits
    }
}

/// Run the server until Ctrl-C.
///
/// Phones of this hub (testing mobil): the paired devices in the config folder, the tailnet name of `tailnet_name`
/// (or `TESTING_DESKTOP_TAILNET`), and Web Push with its key in the config folder.
///
/// # Errors
///
/// The variable names no plain host, or the push key there cannot be read or made.
fn phone_setup(
    dir: &std::path::Path,
    settings: &TestingSettings,
    env: &Env,
) -> Result<crate::phone::Phone, String> {
    let tailnet = match env.get("TESTING_DESKTOP_TAILNET").and_then(|v| v.to_str()) {
        Some(name) if !name.is_empty() => {
            if !crate::phone::plain_tailnet_name(name) {
                return Err(
                    "TESTING_DESKTOP_TAILNET must be a plain host name like pc.example.ts.net"
                        .to_owned(),
                );
            }
            Some(name.to_owned())
        }
        _ => settings.tailnet_name.clone(),
    };
    let transport = appkit_hub::push_net::NetPush::new()?;
    let push = appkit_hub::push::Push::open(dir, Arc::new(transport))
        .map_err(|e| format!("the push of the hub cannot be used: {e}"))?;
    Ok(crate::phone::Phone {
        devices_file: dir.join("devices.json"),
        tailnet,
        push: Some(Arc::new(push)),
    })
}

/// # Errors
///
/// A text for the person at the terminal: what is missing or in the way.
pub async fn serve(opts: ServeOptions) -> Result<(), String> {
    let env = Env::from_process();
    let dir = app_dir(&env)?;
    // A file that cannot be read (or holds a state folder that cannot be used) stops the start: a
    // typo must not turn into results read from somewhere else.
    let file: FileConfig<TestingSettings> =
        appkit_hub::config::load_file(&dir).map_err(|e| e.to_string())?;
    let want = wanted_port(&opts, &file, &env)?;

    if let Some(other) = running_hub(&dir).await {
        return Err(format!(
            "a hub is already running (port {}, process {})",
            other.port, other.pid
        ));
    }

    let ui = match env.path("TESTING_HUB_UI_DIR") {
        Some(dir) => match UiFiles::load(&dir) {
            Ok(files) => files,
            Err(e) => {
                eprintln!(
                    "note: the interface in {} cannot be used: {e}",
                    dir.display()
                );
                UiFiles::embedded()
            }
        },
        None => UiFiles::embedded(),
    };

    // A variable with a path that is not whole stops the start, as config.json does.
    let places = crate::settings::places(&file, &env)?;
    if places.areas.as_ref().is_none_or(|(p, _)| !p.is_file()) {
        eprintln!(
            "note: no areas.json of the data classes was found: no repository is released, the fleet is empty"
        );
    }
    let ask = Ask {
        state_dir: places.state_dir.as_ref().map(|(p, _)| p.clone()),
        // A file that is not there is not named: the engine then says that it has no data classes.
        areas: places
            .areas
            .as_ref()
            .filter(|(p, _)| p.is_file())
            .map(|(p, _)| p.clone()),
        metrics: crate::settings::metrics_for_engine(&places),
        github_token: None,
    };
    let mut origins = Origins {
        state_dir: places.state_dir.as_ref().map(|(_, o)| *o),
        areas: places
            .areas
            .as_ref()
            .filter(|(p, _)| p.is_file())
            .map(|(_, o)| *o),
        ..Origins::default()
    };

    let state = engine_state(&file, &env).await;
    if let Some(note) = &state.note {
        eprintln!("{note}");
    }

    let (github_token, ci) =
        crate::github::resolve(&file.app, &state.status, crate::github::from_gh).await?;
    if ci == CiState::NoToken {
        eprintln!("note: no GitHub token (github_token_file, or `gh auth login`): the CI columns say not asked");
    }
    origins.ci = ci;
    // A red run is started again with gh, with the token the hub reads GitHub with (its own login otherwise).
    let rerun: Option<Arc<dyn crate::rerun::Rerunner>> = matches!(ci, CiState::File | CiState::Gh)
        .then(|| {
            Arc::new(crate::rerun::GhRerun {
                program: "gh".into(),
                token: github_token.clone(),
            }) as Arc<dyn crate::rerun::Rerunner>
        });
    let ask = Ask {
        github_token,
        ..ask
    };
    let phone = Some(phone_setup(&dir, &file.app, &env)?);

    let watch = places
        .state_dir
        .as_ref()
        .filter(|_| state.engine.is_some())
        .map(|(state_dir, _)| Watch {
            state_dir: state_dir.clone(),
            refresh: RefreshOptions::default(),
        });

    let control = Secret::generate(TOKEN_BYTES).map_err(|e| e.to_string())?;
    let instance = Secret::generate(CODE_BYTES).map_err(|e| e.to_string())?;
    let make = |port: u16| Start {
        addr: std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        limits: limits_for(watch.is_some()),
        clock: Arc::new(SystemClock::new()),
        pairing_ttl: Duration::from_secs(60),
        control_secret: control.clone(),
        instance: instance.clone(),
        status: state.status.clone(),
        engine: state.engine.clone(),
        ask: ask.clone(),
        origins,
        watch: watch.clone(),
        step: state.step.clone(),
        ui: ui.clone(),
        warm: true,
        phone: phone.clone(),
        rerun: rerun.clone(),
    };
    // The port is part of the origin, so the wanted one is tried first. When it is taken the server
    // still starts, on another port, and says so.
    let running = match crate::start(make(want)).await {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            if appkit_hub::instance::hello_of(want, &IDENTITY)
                .await
                .is_some()
            {
                return Err(format!("a hub is already running on port {want}"));
            }
            eprintln!("note: port {want} is in use, using a free port; the origin changes");
            crate::start(make(0)).await.map_err(|e| e.to_string())?
        }
        Err(e) => return Err(e.to_string()),
    };
    let port = running.addr().port();

    let record = HubRecord {
        app: IDENTITY.app.to_owned(),
        port,
        pid: std::process::id(),
        version: crate::version().to_owned(),
        instance: running.instance().to_owned(),
    };
    let written = registry::write_secret(&dir, &control.to_hex())
        .and_then(|()| registry::write_record(&dir, &record));
    if let Err(e) = written {
        registry::remove_secret(&dir);
        registry::remove_record(&dir);
        running.shutdown().await;
        return Err(e.to_string());
    }

    println!(
        "{} hub {} on http://127.0.0.1:{port}/",
        IDENTITY.app,
        crate::version()
    );
    println!(
        "run `{} open` for a pairing link; Ctrl-C stops the hub",
        IDENTITY.binary
    );

    wait_for_stop().await;
    registry::remove_record(&dir);
    registry::remove_secret(&dir);
    running.shutdown().await;
    Ok(())
}

/// What `open` says when there is no hub to ask: how to start one and open the page in one go.
pub const START_HINT: &str = "start it and open the page with `testing-hub start --open`";

/// Ask the running hub for a pairing code and return the link for it.
///
/// # Errors
///
/// A text for the person at the terminal.
pub async fn open() -> Result<String, String> {
    let env = Env::from_process();
    let dir = app_dir(&env)?;
    let record = registry::read_record(&dir)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no hub is running (no hub.json); {START_HINT}"))?;
    if record.app != IDENTITY.app {
        return Err("hub.json is not from this app".to_owned());
    }
    let secret = registry::read_secret(&dir)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no control secret: the hub is not running; {START_HINT}"))?;

    // The secret goes only to the process that says it is this hub.
    let hello = client::request(record.port, "GET", &IDENTITY.route("hello"), &[], "")
        .await
        .map_err(|e| format!("the hub does not answer on port {}: {e}", record.port))?;
    let doc: Value = serde_json::from_str(&hello.body).map_err(|_| "not a hub on that port")?;
    if doc["app"] != IDENTITY.app || doc["instance"].as_str() != Some(record.instance.as_str()) {
        return Err(format!(
            "port {} is not the hub of hub.json (a stale file, or another program)",
            record.port
        ));
    }

    let auth = format!("Bearer {secret}");
    let reply = client::request(
        record.port,
        "POST",
        "/api/control/pair",
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &auth),
        ],
        "{}",
    )
    .await
    .map_err(|e| e.to_string())?;
    if reply.status != 200 {
        return Err(format!(
            "the hub refused the request (HTTP {})",
            reply.status
        ));
    }
    let doc: Value = serde_json::from_str(&reply.body).map_err(|e| e.to_string())?;
    let code = doc["code"].as_str().ok_or("the hub sent no code")?;
    Ok(format!("http://127.0.0.1:{}/#pair={code}", record.port))
}

/// Is this the link `open` prints: the loopback address, a port, `/#pair=` and hex, and nothing
/// else a program could take as an option or a path?
#[must_use]
pub fn is_own_link(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://127.0.0.1:") else {
        return false;
    };
    let Some((port, code)) = rest.split_once("/#pair=") else {
        return false;
    };
    !port.is_empty()
        && port.len() <= 5
        && port.bytes().all(|b| b.is_ascii_digit())
        && !code.is_empty()
        && code.len() <= 128
        && code.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Open a link of `open` in the default browser. The link is the hub's own, and nothing else is
/// passed.
///
/// # Errors
///
/// The link is not the hub's own, or the program that opens links could not be started.
pub fn launch(url: &str) -> Result<(), String> {
    if !is_own_link(url) {
        return Err("the link is not the hub's own: not opened".to_owned());
    }
    // The port of the link is the only one the browser helper is allowed to open.
    let port = url
        .strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.split_once('/'))
        .and_then(|(port, _)| port.parse::<u16>().ok())
        .ok_or("the link has no port: not opened")?;
    appkit_hub::browser::open(url, &[port])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_hubs_own_link_is_opened() {
        assert!(is_own_link(
            "http://127.0.0.1:17618/#pair=00ff00ff00ff00ff00ff00ff00ff00ff"
        ));
        for bad in [
            "",
            "https://127.0.0.1:17618/#pair=ab",
            "http://localhost:17618/#pair=ab",
            "http://127.0.0.1:17618/#pair=",
            "http://127.0.0.1:17618/#pair=ab cd",
            "http://127.0.0.1:17618/#pair=ab&x=1",
            "http://127.0.0.1:17618/path#pair=ab",
            "http://127.0.0.1:1/../#pair=ab",
            "http://127.0.0.1:17618@evil.example/#pair=ab",
            "--help",
            "file:///C:/Windows/System32/calc.exe",
            "http://127.0.0.1:17618/#pair=zz",
        ] {
            assert!(!is_own_link(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_link_that_is_not_the_hubs_own_is_not_launched() {
        for bad in [
            "file:///C:/Windows/System32/calc.exe",
            "http://127.0.0.1:17618/other",
            "http://evil.example/#pair=ab",
        ] {
            let err = launch(bad).unwrap_err();
            assert!(err.contains("not opened"), "{bad}: {err}");
        }
    }

    #[test]
    fn the_port_comes_from_the_option_then_the_environment_then_the_file() {
        let file = FileConfig::<TestingSettings> {
            port: Some(20000),
            ..FileConfig::default()
        };
        let env = Env::from_pairs([("TESTING_DESKTOP_PORT", "30000")]);
        let opts = ServeOptions { port: Some(40000) };
        assert_eq!(wanted_port(&opts, &file, &env).unwrap(), 40000);
        let none = ServeOptions::default();
        assert_eq!(wanted_port(&none, &file, &env).unwrap(), 30000);
        assert_eq!(wanted_port(&none, &file, &Env::default()).unwrap(), 20000);
        assert_eq!(
            wanted_port(&none, &FileConfig::default(), &Env::default()).unwrap(),
            IDENTITY.default_port
        );
        // The variables of the other hubs are not read.
        for other in ["TASKS_DESKTOP_PORT", "RULES_DESKTOP_PORT"] {
            let env = Env::from_pairs([(other, "30000")]);
            assert_eq!(
                wanted_port(&none, &FileConfig::default(), &env).unwrap(),
                IDENTITY.default_port,
                "{other}"
            );
        }
    }

    #[test]
    fn a_bad_port_in_the_environment_is_an_error() {
        let none = ServeOptions::default();
        for bad in ["abc", "80", "70000", "-1"] {
            let env = Env::from_pairs([("TESTING_DESKTOP_PORT", bad)]);
            assert!(
                wanted_port(&none, &FileConfig::default(), &env).is_err(),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn only_a_hub_that_watches_holds_a_snapshot_for_long() {
        assert_eq!(
            limits_for(true).snapshot_max_age,
            Some(WATCHED_SNAPSHOT_MAX_AGE)
        );
        assert_eq!(
            limits_for(false).snapshot_max_age,
            Limits::default().snapshot_max_age
        );
    }

    /// A request waits for the engine: a limit below the engine's own cuts off the first read of a fleet.
    #[test]
    fn a_request_may_take_longer_than_the_call_of_the_engine_it_waits_for() {
        for watching in [true, false] {
            assert!(
                limits_for(watching).request_timeout > crate::engine::ENGINE_TIMEOUT,
                "watching: {watching}"
            );
        }
    }

    #[test]
    fn without_an_engine_hello_says_not_ready_and_why() {
        let status = no_engine_status();
        assert!(!status.engine_ready);
        assert_eq!(status.problem, Some("no_engine"));
        assert!(status.capabilities.is_empty());
    }
}
