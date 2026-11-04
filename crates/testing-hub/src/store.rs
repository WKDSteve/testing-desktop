//! The snapshot the hub holds, and the events that tell it changed: the store is the kit's, this
//! module states what the testing hub calls its documents.

pub use appkit_hub::store::{
    spawn_refresher, Cached, Change, RefreshOptions, SnapshotShape, SnapshotStore, StoreOptions,
};

use crate::engine::Calls;

/// The snapshot of the testing engine: the fleet, and the event is `fleet`.
///
/// A row of the fleet has a `key` and no `id` or `etag`, and the store reads each entry of the list
/// it is pointed at for those: pointed at `repos` it would find the document unreadable, and lose its
/// digest and with it the ETag. So it is pointed at a list no fleet holds. An event of this hub
/// therefore names no repository (its lists are empty); it says that the fleet changed, and the page
/// reads again.
pub const SHAPE: SnapshotShape = SnapshotShape {
    call: Calls::Fleet,
    items_key: "no-list-of-ids-in-a-fleet",
    event: "fleet",
    transitional: &[],
};

#[cfg(test)]
mod tests {
    use super::*;

    const FLEET: &str = include_str!("../tests/fixtures/golden/fleet.json");

    /// A client listens for `event: fleet`: part of the contract with the page.
    #[test]
    fn the_names_of_the_testing_contract_stay() {
        assert_eq!(SHAPE.event, "fleet");
        assert_eq!(SHAPE.call, Calls::Fleet);
        assert!(SHAPE.transitional.is_empty());
    }

    #[test]
    fn the_fleet_has_no_list_under_the_key_of_the_shape() {
        let doc: serde_json::Value = serde_json::from_str(FLEET).unwrap();
        assert!(doc.get(SHAPE.items_key).is_none());
    }
}
