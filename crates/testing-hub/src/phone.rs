//! testing mobil on the hub's side: which routes a paired phone may use, at which level, and the guard rules that say
//! so (`MOB-02`, `MOB-03`, rules of appkit's `docs/MOBILE.md`).
//!
//! A phone reads the fleet, one repository and its findings, and `about`; it may start one thing, a rerun of the
//! failed jobs of a red CI run of the default branch (`ci-rerun`), at the level `trigger` and with a confirmation on
//! the phone for each request. Everything else stays on the PC: a stepped run of a test sheet (it runs code), the event
//! stream, and above all `testing ci trust`, releasing commits of others, and any command of the person's choice. No
//! route of this hub does those, and none is released to a phone: the tests hold [`DEVICE_ROUTES`] to that.

use std::path::PathBuf;
use std::sync::Arc;

use appkit_hub::push::Push;
use appkit_hub::{GuardRules, Level};

use crate::IDENTITY;

/// The routes a phone may use, by name below `/api/testing/`, with the level each needs. The only one of the level
/// `trigger` is `ci-rerun`.
pub const DEVICE_ROUTES: &[(&str, Level)] = &[
    ("fleet", Level::Read),
    ("repo", Level::Read),
    ("finding", Level::Read),
    ("about", Level::Read),
    ("ci-rerun", Level::Trigger),
];

/// The kind of event a phone subscribes to for a red default branch.
pub const CI_RED: &str = "ci_red";

/// What the hub needs for phones: where it keeps the paired devices, the name of the PC in the tailnet (when
/// Tailscale Serve forwards to the hub), and its Web Push.
#[derive(Clone)]
pub struct Phone {
    pub devices_file: PathBuf,
    pub tailnet: Option<String>,
    pub push: Option<Arc<Push>>,
}

/// The guard rules of the hub: `hello` without a session; with phones, the devices, the tailnet name, the routes of
/// [`DEVICE_ROUTES`] and push.
#[must_use]
pub fn guard_rules(phone: Option<&Phone>) -> GuardRules {
    // The two POST routes of a stepped run (`sheet-step/start`, `sheet-step/command`) and `ci-rerun` have the strict
    // limit of a mutation; no route needs the coarse one of a write that only its body tells from a dry run.
    let rules = GuardRules::new(IDENTITY.route("hello"), []);
    let Some(phone) = phone else {
        return rules;
    };
    let mut rules = rules.with_devices(phone.devices_file.clone());
    if let Some(name) = &phone.tailnet {
        rules = rules.with_tailnet_name(name.clone());
    }
    for (name, level) in DEVICE_ROUTES {
        rules = rules.with_device_route(IDENTITY.route(name), *level);
    }
    if let Some(push) = &phone.push {
        rules = rules.with_push(Arc::clone(push));
    }
    rules
}

/// Is `name` a plain tailnet name of a PC (`pc.example.ts.net`): lower-case labels of letters, digits and `-`, at
/// least two, the last not all digits? What `with_tailnet_name` takes, checked when the settings are read so that a
/// typo stops the start with a word instead of a panic.
#[must_use]
pub fn plain_tailnet_name(name: &str) -> bool {
    let labels: Vec<&str> = name.split('.').collect();
    name.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|l| {
            (1..=63).contains(&l.len())
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
        && labels
            .last()
            .is_some_and(|tld| tld.bytes().any(|b| b.is_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Never from the phone" (decided 2026-11-11): no route a phone reaches trusts, releases, steps a sheet or runs a
    /// command of the person's choice, and the one that starts something is `ci-rerun`, at the level `trigger`.
    #[test]
    fn a_phone_reaches_reads_and_the_rerun_and_never_trust_release_sheets_or_commands() {
        let served: Vec<&str> = crate::ROUTES.iter().map(|(n, _)| *n).collect();
        for (name, level) in DEVICE_ROUTES {
            assert!(served.contains(name), "{name} is a route of the hub");
            for word in [
                "trust", "release", "sheet", "step", "command", "exec", "run-", "shell",
            ] {
                assert!(!name.contains(word), "{name} holds {word}");
            }
            assert_eq!(
                *level == Level::Trigger,
                *name == "ci-rerun",
                "{name}: only the rerun triggers"
            );
        }
        // And the hub has no route of that kind at all, released or not.
        for name in served {
            for word in ["trust", "release", "approve", "exec", "shell"] {
                assert!(!name.contains(word), "{name}");
            }
        }
        let names: Vec<&str> = DEVICE_ROUTES.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["fleet", "repo", "finding", "about", "ci-rerun"]);
    }

    #[test]
    fn a_tailnet_name_is_a_plain_host_name() {
        assert!(plain_tailnet_name("pc.example.ts.net"));
        for bad in [
            "",
            "pc",
            "PC.ts.net",
            "pc.ts.net:443",
            "https://pc.ts.net",
            "10.0.0.1",
            "pc..ts.net",
        ] {
            assert!(!plain_tailnet_name(bad), "{bad}");
        }
    }
}
