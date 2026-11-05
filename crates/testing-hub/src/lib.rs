//! The hub of testing-desktop, on `appkit-hub`: its names, its settings, the engine seam to
//! testing.nvim (`testing-export/1`, read only) and the routes a session reads (`fleet`, `repo`,
//! `finding`, `events`, `about`). The fleet is a snapshot that a watcher of the state folder renews.
//! The commands of the program (`serve`, `start`, `stop`, `open`, `autostart`, `install`,
//! `uninstall`) are in `app` and `background`.

use std::net::SocketAddr;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use appkit_core::authn::{Clock, Secret};
use appkit_core::identity::AppIdentity;
use appkit_hub::{serve, GuardRules, HubStatus, Limits, RunningServer, ServeParts};

mod answer;
pub mod app;
pub mod background;
pub mod engine;
pub mod github;
mod live;
pub mod places;
mod routes;
mod runtime;
pub mod settings;
pub mod store;
pub mod ui;
pub mod watch;

pub use routes::{MakeRoute, Shared, ROUTES};
pub use runtime::{Origins, Reading, TestingRuntime, Watch};

pub use appkit_hub::{client, loginitems};

/// The names of the app: its config folder, its program, its variables, its port and its routes.
pub const IDENTITY: AppIdentity = AppIdentity {
    app: "testing-desktop",
    binary: "testing-hub",
    env_prefix: "TESTING_DESKTOP",
    title: "Testing",
    default_port: 17618,
    route_ns: "testing",
};

/// What the login entries and the launcher say about this app.
pub const LOGIN: appkit_hub::loginitems::LoginApp<'static> = appkit_hub::loginitems::LoginApp {
    identity: &IDENTITY,
    summary: "the test and CI results",
};

/// The version of this build, as `testing-hub --version` and `about` say it.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// What a build that is not a release says about itself (the commit it was made from): set by the
/// workflow of the rolling build through `TESTING_HUB_BUILD`, absent in a release and in a local build.
pub const BUILD: Option<&str> = option_env!("TESTING_HUB_BUILD");

/// The line `--version` prints: `testing-hub 0.0.0`, and `testing-hub 0.0.0 (build abc1234)` for a
/// rolling build. A release is exactly the first form (the release workflow compares it with the tag).
#[must_use]
pub fn version_line() -> String {
    version_line_for(BUILD)
}

/// [`version_line`] for a given build (so that both forms are testable: `BUILD` is fixed at compile time).
#[must_use]
pub fn version_line_for(build: Option<&str>) -> String {
    match build {
        Some(build) if !build.is_empty() => {
            format!("{} {} (build {build})", IDENTITY.binary, version())
        }
        _ => format!("{} {}", IDENTITY.binary, version()),
    }
}

/// The routes that need a session, by name under `/api/testing/`: every route of [`ROUTES`], so the
/// security matrix tries each one that is served.
pub static DATA_ROUTES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| ROUTES.iter().map(|(name, _)| *name).collect());

/// What a start of the hub is given; the rest are the hub's own names.
pub struct Start {
    /// Must be loopback; port 0 picks a free port.
    pub addr: SocketAddr,
    pub limits: Limits,
    pub clock: Arc<dyn Clock>,
    pub pairing_ttl: Duration,
    pub control_secret: Secret,
    pub instance: Secret,
    pub status: HubStatus,
    /// The engine the routes ask; `None`: the hub is up and every data route says the engine is
    /// not ready.
    pub engine: Option<Arc<dyn engine::Engine>>,
    /// What every question carries (the state folder, the `areas.json`), and where they came from.
    pub ask: engine::Ask,
    pub origins: Origins,
    /// The state folder to watch for new results, with the timings of the scan. `None`: nothing is
    /// watched, the snapshot of the fleet changes when it is too old (`limits.snapshot_max_age`).
    pub watch: Option<Watch>,
    /// The files of the page; [`ui::UiFiles::none`] serves the placeholder page of the core.
    pub ui: ui::UiFiles,
}

/// Starts the hub.
///
/// # Errors
///
/// When the address is not loopback or cannot be bound.
pub async fn start(s: Start) -> std::io::Result<RunningServer> {
    let max_age = s.limits.snapshot_max_age;
    serve(
        ServeParts {
            addr: s.addr,
            limits: s.limits,
            clock: s.clock,
            pairing_ttl: s.pairing_ttl,
            control_secret: s.control_secret,
            instance: s.instance,
            app: IDENTITY.app,
            version: version(),
            build_id: BUILD,
            // Nothing writes: no route has the limit of the writes.
            guard_rules: GuardRules::new(IDENTITY.route("hello"), []),
            ui: Arc::new(s.ui),
            runtime: TestingRuntime::reading(
                s.status,
                Reading {
                    engine: s.engine,
                    ask: s.ask,
                    origins: s.origins,
                    max_age,
                    watch: s.watch,
                },
            ),
            // There is one repositories root and no list to choose from, so this is never called.
            build: Arc::new(|()| TestingRuntime::new(app::no_engine_status())),
            vaults: None,
        },
        routes::app_routes,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_semver_shaped() {
        assert_eq!(version().split('.').count(), 3);
    }

    #[test]
    fn a_rolling_build_names_its_commit_and_a_release_does_not() {
        let release = format!("testing-hub {}", version());
        assert_eq!(version_line_for(None), release);
        assert_eq!(version_line_for(Some("")), release);
        assert_eq!(
            version_line_for(Some("abc1234")),
            format!("{release} (build abc1234)")
        );
    }
}
