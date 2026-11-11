//! Where the files are that the engine cannot find by itself in its allow-listed environment: the
//! state folder of testing.nvim (`stdpath('state')` of the editor that runs the tests) and the
//! `areas.json` of the data classes (the config folder of tasks-desktop, as lib.nvim
//! `data_classes.config_dir` looks for it). The hub works them out from its own environment, once
//! at the start, and a setting of `config.json` overrides each.

use std::path::PathBuf;

use appkit_core::identity::AppIdentity;
use appkit_hub::env::Env;

/// The names of tasks-desktop, whose config folder keeps `areas.json` (only `app` and `env_prefix`
/// are used: the folder and `TASKS_DESKTOP_CONFIG_DIR`).
const TASKS: AppIdentity = AppIdentity {
    app: "tasks-desktop",
    binary: "tasks-hub",
    env_prefix: "TASKS_DESKTOP",
    title: "Tasks",
    default_port: 17615,
    route_ns: "tasks",
};

/// The file of the data classes of this device.
pub const AREAS_FILE: &str = "areas.json";

/// `stdpath('state')` of Neovim for this environment: `NVIM_APPNAME` (default `nvim`) below
/// `%LOCALAPPDATA%` with `-data` on Windows, below `$XDG_STATE_HOME` or `~/.local/state` elsewhere.
/// `None` when the variables it needs are not set.
#[must_use]
pub fn nvim_state_dir(env: &Env) -> Option<PathBuf> {
    let app = env
        .get("NVIM_APPNAME")
        .map(|v| v.to_string_lossy().into_owned())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "nvim".to_owned());
    if cfg!(windows) {
        env.path("LOCALAPPDATA")
            .map(|b| b.join(format!("{app}-data")))
    } else {
        env.path("XDG_STATE_HOME")
            .or_else(|| env.path("HOME").map(|h| h.join(".local").join("state")))
            .map(|b| b.join(app))
    }
}

/// The `metrics.jsonl` of the heavy runs of this machine (testing.nvim `testing.metrics`): the
/// data folder of the user, as the engine finds it in a full environment (`%LOCALAPPDATA%` on
/// Windows, else `$XDG_DATA_HOME` or `~/.local/share`), below `testing/`. `None` when
/// `TESTING_METRICS=off` or the variables it needs are not set. `TESTING_METRICS_FILE` is picked
/// before it by [`crate::settings::places`].
#[must_use]
pub fn metrics_file(env: &Env) -> Option<PathBuf> {
    if env.get("TESTING_METRICS").is_some_and(|v| v == "off") {
        return None;
    }
    let base = if cfg!(windows) {
        env.path("LOCALAPPDATA")
    } else {
        None
    };
    base.or_else(|| env.path("XDG_DATA_HOME"))
        .or_else(|| env.path("HOME").map(|h| h.join(".local").join("share")))
        .map(|d| d.join("testing").join("metrics.jsonl"))
}

/// The `areas.json` of this device: in the config folder of tasks-desktop. `None` when there is
/// no config folder.
#[must_use]
pub fn areas_file(env: &Env) -> Option<PathBuf> {
    appkit_hub::config::config_dir(env, &TASKS).map(|d| d.join(AREAS_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_metrics_file_is_the_one_the_engine_writes() {
        let env = Env::from_pairs([
            ("LOCALAPPDATA", "/local"),
            ("XDG_DATA_HOME", "/xdg"),
            ("HOME", "/home/u"),
        ]);
        let want = if cfg!(windows) { "/local" } else { "/xdg" };
        assert_eq!(
            metrics_file(&env),
            Some(PathBuf::from(want).join("testing").join("metrics.jsonl"))
        );
        let home = Env::from_pairs([("HOME", "/home/u")]);
        assert_eq!(
            metrics_file(&home),
            Some(
                PathBuf::from("/home/u")
                    .join(".local")
                    .join("share")
                    .join("testing")
                    .join("metrics.jsonl")
            )
        );
        let off = Env::from_pairs([("LOCALAPPDATA", "/local"), ("TESTING_METRICS", "off")]);
        assert_eq!(metrics_file(&off), None);
        assert_eq!(
            metrics_file(&Env::from_pairs::<[(&str, &str); 0], _, _>([])),
            None
        );
    }

    #[test]
    fn the_state_folder_follows_nvim_appname_and_the_platform() {
        let env = Env::from_pairs([
            ("LOCALAPPDATA", "/local"),
            ("XDG_STATE_HOME", "/xdg/state"),
            ("HOME", "/home/u"),
        ]);
        let want = if cfg!(windows) {
            PathBuf::from("/local").join("nvim-data")
        } else {
            PathBuf::from("/xdg/state").join("nvim")
        };
        assert_eq!(nvim_state_dir(&env), Some(want));
        let named = Env::from_pairs([
            ("LOCALAPPDATA", "/local"),
            ("HOME", "/home/u"),
            ("NVIM_APPNAME", "other"),
        ]);
        let want = if cfg!(windows) {
            PathBuf::from("/local").join("other-data")
        } else {
            PathBuf::from("/home/u")
                .join(".local")
                .join("state")
                .join("other")
        };
        assert_eq!(nvim_state_dir(&named), Some(want));
        assert_eq!(nvim_state_dir(&Env::default()), None);
    }

    #[test]
    fn the_areas_file_is_in_the_config_folder_of_tasks_desktop() {
        let env = Env::from_pairs([("TASKS_DESKTOP_CONFIG_DIR", "/cfg/tasks")]);
        assert_eq!(
            areas_file(&env),
            Some(PathBuf::from("/cfg/tasks").join("areas.json"))
        );
        // The variable of this app does not move the file of the other.
        let own = Env::from_pairs([("TESTING_DESKTOP_CONFIG_DIR", "/cfg/testing")]);
        assert_ne!(
            areas_file(&own),
            Some(PathBuf::from("/cfg/testing").join("areas.json"))
        );
        assert_eq!(areas_file(&Env::default()), None);
    }
}
