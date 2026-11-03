//! What the hub keeps while it runs. For now that is the status `hello` reports; the engine, the
//! snapshot of the fleet and the watcher of the state folder join it with the engine seam.

use appkit_hub::{HubStatus, Runtime};

/// What the hub keeps for the repositories it reads.
pub struct TestingRuntime {
    pub(crate) status: HubStatus,
}

impl TestingRuntime {
    #[must_use]
    pub fn new(status: HubStatus) -> Self {
        Self { status }
    }
}

impl Runtime for TestingRuntime {
    type Target = ();

    fn status(&self) -> &HubStatus {
        &self.status
    }

    fn stop(&self) {}
}
