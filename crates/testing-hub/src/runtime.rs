//! What the hub keeps while it runs: the status `hello` reports, the engine, the request every call
//! carries and where its parts came from, the snapshot of the fleet with its live updates, and the
//! watcher of the state folder.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use appkit_hub::{HubStatus, Runtime};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::engine::{Ask, Engine};
use crate::github::CiState;
use crate::settings::Origin;
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

/// The event streams that are open, by session token.
#[derive(Default)]
pub(crate) struct StreamCounter {
    by_token: Mutex<HashMap<String, usize>>,
}

/// One open stream; the place is given back when it is dropped.
pub(crate) struct StreamGuard {
    counter: Arc<StreamCounter>,
    token: String,
}

impl StreamCounter {
    pub(crate) fn acquire(self: &Arc<Self>, token: &str, max: usize) -> Option<StreamGuard> {
        let mut map = self
            .by_token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let open = map.entry(token.to_owned()).or_insert(0);
        if *open >= max {
            return None;
        }
        *open += 1;
        Some(StreamGuard {
            counter: Arc::clone(self),
            token: token.to_owned(),
        })
    }
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        let mut map = self
            .counter
            .by_token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(open) = map.get_mut(&self.token) {
            *open = open.saturating_sub(1);
            if *open == 0 {
                map.remove(&self.token);
            }
        }
    }
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
        Self {
            status,
            engine,
            ask,
            origins,
            store,
            streams: Arc::default(),
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
        let mut jobs = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for job in jobs.drain(..) {
            job.abort();
        }
    }
}
