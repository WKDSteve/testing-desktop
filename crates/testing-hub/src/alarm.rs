//! A notice to the phones when the default branch of a repository turns red (`ci_red`, Web Push of appkit).
//!
//! While a device has subscribed, the hub reads the fleet every few minutes (the engine asks GitHub at most once a
//! minute per repository) and compares the CI state of each repository with the one before: a change from not red to
//! red is a notice. The first read only sets what is known; a hub that starts does not tell old news. A notice names
//! the kind and the key of the repository, whose class the fleet already released (the hub asks the engine for the
//! export form, filtered by the data classes); never its name or a case.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use appkit_hub::push::Push;
use appkit_hub::DataClass;
use serde_json::Value;

use crate::phone::CI_RED;
use crate::store::SnapshotStore;

/// How often the fleet is read while a device waits for a notice.
pub const EVERY: Duration = Duration::from_secs(5 * 60);

/// The states that count as red.
const RED: [&str; 4] = ["failure", "rejected", "cancelled", "no_runner"];

/// Which repositories of a fleet are red now, by key.
#[must_use]
pub fn red_by_key(fleet: &Value) -> BTreeMap<String, bool> {
    fleet["repos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let key = row["key"].as_str()?;
            let state = row["ci_remote"]["state"].as_str().unwrap_or_default();
            Some((key.to_owned(), RED.contains(&state)))
        })
        .collect()
}

/// The keys that turned red since `before` (a key not known before is not news).
#[must_use]
pub fn turned_red(before: &BTreeMap<String, bool>, now: &BTreeMap<String, bool>) -> Vec<String> {
    now.iter()
        .filter(|(key, red)| **red && before.get(*key) == Some(&false))
        .map(|(key, _)| key.clone())
        .collect()
}

/// The watch: read, compare, tell. Ends with the hub (the task is aborted).
pub fn spawn(
    store: Arc<SnapshotStore>,
    push: Arc<Push>,
    every: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut known: Option<BTreeMap<String, bool>> = None;
        loop {
            tokio::time::sleep(every).await;
            if !push.has_subscribers(CI_RED) {
                known = None;
                continue;
            }
            if store.refresh("timer").await.is_err() {
                continue;
            }
            let Some(cached) = store.latest() else {
                continue;
            };
            let Ok(fleet) = serde_json::from_slice::<Value>(&cached.text) else {
                continue;
            };
            let now = red_by_key(&fleet);
            if let Some(before) = &known {
                for key in turned_red(before, &now) {
                    let _ = push
                        .notify(CI_RED, Some((&key, DataClass::Internal)), true)
                        .await;
                }
            }
            known = Some(now);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fleet(rows: &[(&str, &str)]) -> Value {
        json!({ "repos": rows.iter().map(|(k, s)| json!({ "key": k, "ci_remote": { "state": s } })).collect::<Vec<_>>() })
    }

    #[test]
    fn only_a_change_to_red_is_news() {
        let before = red_by_key(&fleet(&[
            ("a", "success"),
            ("b", "failure"),
            ("c", "running"),
        ]));
        let now = red_by_key(&fleet(&[
            ("a", "failure"),
            ("b", "failure"),
            ("c", "rejected"),
            ("d", "failure"),
        ]));
        assert_eq!(turned_red(&before, &now), ["a", "c"]);
        assert!(turned_red(&now, &now).is_empty());
        let row_without_remote = json!({ "repos": [{ "key": "e", "ci_remote": null }] });
        assert_eq!(red_by_key(&row_without_remote).get("e"), Some(&false));
    }
}
