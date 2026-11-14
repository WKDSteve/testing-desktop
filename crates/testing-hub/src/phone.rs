//! testing mobil on the hub's side: which routes a paired phone may use, at which level, and the guard rules that say
//! so (`MOB-02`, `MOB-03`, rules of appkit's `docs/MOBILE.md`).
//!
//! A phone reads the fleet, one repository and its findings, and `about`; it may start two things, each at the level
//! `trigger` and with a confirmation on the phone for each request: a rerun of the failed jobs of a red CI run of the
//! default branch (`ci-rerun`), and a run of the test sheets bound to a task (`run/task`, decided 2026-11-11: new runs
//! only in the context of a task, run on the PC), which the hub starts only when a sheet of the task's repository
//! names the task. It reads where a run stands (`run`, cut for a device) and which sheets a task has
//! (`run/task-sheets`). Everything else stays on the PC: a stepped run of a test sheet, a run of a whole suite or of
//! a file (`run/start`), the event stream, and above all the release of a commit for the self-hosted runner (the
//! assistant of `release/*`, the engine's `testing ci release`: a person looks at what a commit changes, on the PC)
//! and any command of the person's choice. No route of this hub runs such a command or trusts a root, and none of
//! the release assistant is released to a phone: the tests hold [`DEVICE_ROUTES`] to that.

use std::path::PathBuf;
use std::sync::Arc;

use appkit_hub::push::Push;
use appkit_hub::{GuardRules, Level};

use crate::IDENTITY;

/// The routes a phone may use, by name below `/api/testing/`, with the level each needs. Those of the level
/// `trigger` are [`TRIGGERS`].
pub const DEVICE_ROUTES: &[(&str, Level)] = &[
    ("fleet", Level::Read),
    ("repo", Level::Read),
    ("finding", Level::Read),
    ("about", Level::Read),
    ("ci-rerun", Level::Trigger),
    ("run", Level::Read),
    ("run/task-sheets", Level::Read),
    ("run/task", Level::Trigger),
];

/// The routes that start something from a phone: the rerun of a red CI run, and the run of the sheets of a task.
pub const TRIGGERS: &[&str] = &["ci-rerun", "run/task"];

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
    // The POST routes of a stepped run (`sheet-step/start`, `sheet-step/command`), of a run (`run/start`,
    // `run/task`) and `ci-rerun` have the strict limit of a mutation; no route needs the coarse one of a write that only its body tells from a dry run.
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
/// least two, the last not all digits, with an optional `:<port>` other than 443 when Tailscale Serve forwards another
/// HTTPS port to this hub (`pc.example.ts.net:8443`, so that a second hub of the PC has 443)? What
/// `with_tailnet_name` takes (appkit's own rule), checked when the settings are read so that a typo stops the start
/// with a word instead of a panic.
#[must_use]
pub fn plain_tailnet_name(name: &str) -> bool {
    appkit_core::authn::tailnet_name_ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Never from the phone" (decided 2026-11-11): no route a phone reaches trusts, releases, steps a sheet, runs a
    /// suite or a file, or runs a command of the person's choice; the two that start something are `ci-rerun` and
    /// `run/task` (the sheets of a task), at the level `trigger`.
    #[test]
    fn a_phone_reaches_reads_the_rerun_and_a_tasks_sheets_and_never_trust_release_steps_or_commands(
    ) {
        let served: Vec<&str> = crate::ROUTES.iter().map(|(n, _)| *n).collect();
        for (name, level) in DEVICE_ROUTES {
            assert!(served.contains(name), "{name} is a route of the hub");
            for word in [
                "trust",
                "release",
                "sheet-step",
                "step",
                "command",
                "exec",
                "run-",
                "shell",
                "start",
            ] {
                assert!(!name.contains(word), "{name} holds {word}");
            }
            assert_eq!(
                *level == Level::Trigger,
                TRIGGERS.contains(name),
                "{name}: only the rerun and a task's sheets trigger"
            );
        }
        // The hub has no route that trusts a root or runs a command at all, released or not.
        for name in &served {
            for word in ["trust", "approve", "exec", "shell"] {
                assert!(!name.contains(word), "{name}");
            }
        }
        // The release of a commit for the runner is the assistant's, on the PC: every route of it is served, and a
        // phone reaches none.
        let assistant: Vec<&&str> = served.iter().filter(|n| n.contains("release")).collect();
        assert_eq!(
            assistant,
            [
                &"release",
                &"release/repos",
                &"release/commits",
                &"release/show",
                &"release/outcome"
            ]
        );
        let names: Vec<&str> = DEVICE_ROUTES.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            [
                "fleet",
                "repo",
                "finding",
                "about",
                "ci-rerun",
                "run",
                "run/task-sheets",
                "run/task"
            ]
        );
        // The run of a suite or a file, and a stepped sheet, are served and stay on the PC.
        for pc_only in [
            "run/start",
            "sheet-step/start",
            "sheet-step/command",
            "events",
            "release",
            "release/repos",
            "release/commits",
            "release/show",
            "release/outcome",
        ] {
            assert!(served.contains(&pc_only), "{pc_only}");
            assert!(!names.contains(&pc_only), "{pc_only}");
        }
    }

    #[test]
    fn a_tailnet_name_is_a_plain_host_name() {
        assert!(plain_tailnet_name("pc.example.ts.net"));
        assert!(plain_tailnet_name("pc.example.ts.net:8443"));
        for bad in [
            "",
            "pc",
            "PC.ts.net",
            "pc.ts.net:443",
            "pc.ts.net:0",
            "pc.ts.net:",
            "https://pc.ts.net",
            "10.0.0.1",
            "pc..ts.net",
        ] {
            assert!(!plain_tailnet_name(bad), "{bad}");
        }
    }
}
