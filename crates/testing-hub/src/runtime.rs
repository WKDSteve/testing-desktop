//! What the hub keeps while it runs: the status `hello` reports, the engine, the request every call
//! carries and where its parts came from, the snapshot of the fleet with its live updates, and the
//! watcher of the state folder.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use appkit_hub::streams::StreamCounter;
use appkit_hub::{HubStatus, Runtime};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::engine::{Ask, Engine};
use crate::github::CiState;
use crate::settings::Origin;
use crate::sheetstep::{SheetStep, StepProgram};
use crate::store::{self, RefreshOptions, SnapshotStore, StoreOptions, SHAPE};
use crate::watch::{self, Hint, Watcher};

/// What the hub keeps for the repositories it reads.
pub struct TestingRuntime {
    pub(crate) status: HubStatus,
    /// `None`: the hub is up and every data route says the engine is not ready.
    pub(crate) engine: Option<Arc<dyn Engine>>,
    pub(crate) ask: Ask,
    /// Where the state folder and the `areas.json` came from (`about` says this, never the path).
    pub(crate) origins: Origins,
    /// The fleet, asked of the engine once per change and not per request.
    pub(crate) store: Arc<SnapshotStore>,
    /// The event streams that are open, by session token.
    pub(crate) streams: Arc<StreamCounter>,
    /// The one stepped run of a test sheet, and what the page may ask of it.
    pub(crate) sheet_step: Arc<SheetStep>,
    /// How a red CI run is started again; `None`: it is not.
    pub(crate) rerun: Option<Arc<dyn crate::rerun::Rerunner>>,
    /// The refresher; ended when the hub stops.
    jobs: Mutex<Vec<JoinHandle<()>>>,
    // Kept alive: the refresher ends when every sender of hints is gone.
    _hints: Option<mpsc::UnboundedSender<Hint>>,
    _watcher: Option<Watcher>,
}

/// Where each part of [`Ask`] came from; `None`: the hub knows none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Origins {
    pub state_dir: Option<Origin>,
    pub areas: Option<Origin>,
    /// Whether the hub asks GitHub, and why not.
    pub ci: CiState,
}

/// What the hub watches: the state folder, and how often it scans.
#[derive(Debug, Clone)]
pub struct Watch {
    pub state_dir: PathBuf,
    pub refresh: RefreshOptions,
}

/// What the runtime is started with for the engine.
pub struct Reading {
    pub engine: Option<Arc<dyn Engine>>,
    pub ask: Ask,
    pub origins: Origins,
    /// How old a held snapshot may be when a request finds it (`Limits::snapshot_max_age`).
    pub max_age: Option<Duration>,
    /// `None`: nothing is watched, the snapshot changes when it is too old.
    pub watch: Option<Watch>,
    /// How a stepped run of a sheet is started; `None`: the page cannot start one.
    pub step: Option<StepProgram>,
    /// Read the fleet once at the start (`Start::warm`).
    pub warm: bool,
    /// How a red CI run is started again (`ci-rerun`).
    pub rerun: Option<Arc<dyn crate::rerun::Rerunner>>,
    /// Web Push of the hub: with it (and an engine) the hub tells phones when a default branch turns red.
    pub push: Option<Arc<appkit_hub::push::Push>>,
}

impl TestingRuntime {
    /// A hub without an engine.
    #[must_use]
    pub fn new(status: HubStatus) -> Self {
        Self::reading(
            status,
            Reading {
                engine: None,
                ask: Ask::default(),
                origins: Origins::default(),
                max_age: None,
                watch: None,
                step: None,
                warm: false,
                rerun: None,
                push: None,
            },
        )
    }

    /// With the engine the routes ask, what they ask it with, and the watcher that tells them when.
    /// Inside the runtime of the hub: the refresher is a task.
    #[must_use]
    pub fn reading(status: HubStatus, reading: Reading) -> Self {
        let Reading {
            engine,
            ask,
            origins,
            max_age,
            watch,
            step,
            warm,
            rerun,
            push,
        } = reading;
        let store = SnapshotStore::with_params(
            engine.clone(),
            SHAPE,
            StoreOptions {
                max_age,
                ..StoreOptions::default()
            },
            Some(ask.fleet()),
        );
        // The watcher only says "look again"; the refresher does the looking.
        let (mut hints, mut watcher, mut jobs) = (None, None, Vec::new());
        if let Some(watch) = watch.filter(|_| engine.is_some()) {
            let (tx, rx) = mpsc::unbounded_channel();
            match watch::start(&watch.state_dir, tx.clone()) {
                Ok(started) => watcher = Some(started),
                // Not fatal: the timer still scans while a page listens.
                Err(e) => {
                    eprintln!(
                        "note: the state folder cannot be watched ({e}); scanning on a timer only"
                    );
                }
            }
            jobs.push(store::spawn_refresher(
                Arc::clone(&store),
                rx,
                watch.refresh,
            ));
            hints = Some(tx);
        }
        // The first read of the fleet starts with the hub, not with the first page: a page that comes
        // while it runs waits for the same read (one fetch at a time), one that comes later finds it done.
        // A failure is not kept: the next request asks again and says what went wrong.
        if warm && engine.is_some() {
            let store = Arc::clone(&store);
            jobs.push(tokio::spawn(async move {
                let _ = store.current().await;
            }));
        }
        // A notice to the phones when a default branch turns red: only with an engine to read the fleet.
        if let Some(push) = push.filter(|_| engine.is_some()) {
            jobs.push(crate::alarm::spawn(
                Arc::clone(&store),
                push,
                crate::alarm::EVERY,
            ));
        }
        // Only with an engine: the program of a run is the engine's own.
        let sheet_step = Arc::new(SheetStep::new(step.filter(|_| engine.is_some())));
        Self {
            status,
            engine,
            ask,
            origins,
            store,
            streams: Arc::default(),
            sheet_step,
            rerun,
            jobs: Mutex::new(jobs),
            _hints: hints,
            _watcher: watcher,
        }
    }
}

impl Runtime for TestingRuntime {
    type Target = ();

    fn status(&self) -> &HubStatus {
        &self.status
    }

    fn stop(&self) {
        // A run under way ends with the hub: nothing stays behind.
        self.sheet_step.stop();
        let mut jobs = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for job in jobs.drain(..) {
            job.abort();
        }
    }
}
