//! What the hub keeps while it runs: the status `hello` reports, the engine, the request every call
//! carries and where its parts came from. The snapshot of the fleet with its live updates and the
//! watcher of the state folder join it in a later step; until then every read asks the engine.

use std::sync::Arc;

use appkit_hub::{HubStatus, Runtime};

use crate::engine::{Ask, Engine};
use crate::settings::Origin;

/// What the hub keeps for the repositories it reads.
pub struct TestingRuntime {
    pub(crate) status: HubStatus,
    /// `None`: the hub is up and every data route says the engine is not ready.
    pub(crate) engine: Option<Arc<dyn Engine>>,
    pub(crate) ask: Ask,
    /// Where the state folder and the `areas.json` came from (`about` says this, never the path).
    pub(crate) origins: Origins,
}

/// Where each part of [`Ask`] came from; `None`: the hub knows none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Origins {
    pub state_dir: Option<Origin>,
    pub areas: Option<Origin>,
}

impl TestingRuntime {
    #[must_use]
    pub fn new(status: HubStatus) -> Self {
        Self {
            status,
            engine: None,
            ask: Ask::default(),
            origins: Origins::default(),
        }
    }

    /// With the engine the routes ask, and what they ask it with.
    #[must_use]
    pub fn with_engine(
        mut self,
        engine: Option<Arc<dyn Engine>>,
        ask: Ask,
        origins: Origins,
    ) -> Self {
        self.engine = engine;
        self.ask = ask;
        self.origins = origins;
        self
    }
}

impl Runtime for TestingRuntime {
    type Target = ();

    fn status(&self) -> &HubStatus {
        &self.status
    }

    fn stop(&self) {}
}
