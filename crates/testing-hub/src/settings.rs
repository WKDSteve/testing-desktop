//! The settings of this app in `config.json`, beside the keys of the kit (vault, nvim, engine_dir, port).
//!
//! `state_dir` is the state folder of testing.nvim the hub reads the results from (`stdpath('state')` of
//! the editor that runs the tests). The engine is a child with an allow-listed environment, so it cannot
//! find that folder by itself; the hub hands it over as a parameter of every call, never through the
//! environment. It is a statement of the person about this device, never of the page.
//! `TESTING_DESKTOP_STATE_DIR` overrides the file; without either the hub works it out
//! (`places::nvim_state_dir`).
//!
//! `areas` is the `areas.json` of the data classes of this device, for the same reason; without it the
//! file in the config folder of tasks-desktop (`places::areas_file`). `TESTING_DESKTOP_AREAS`
//! overrides the file.
//!
//! `github_token_file` holds the token the hub asks GitHub with (`github`); without it the hub takes
//! `gh auth token` once. `ask_github = false` asks GitHub with neither.

use std::path::{Path, PathBuf};

use appkit_hub::config::{AppSettings, FileConfig};
use appkit_hub::env::Env;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TestingSettings {
    /// The state folder of testing.nvim; none: the hub works it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_dir: Option<PathBuf>,
    /// The `areas.json` of the data classes; none: the one of tasks-desktop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub areas: Option<PathBuf>,
    /// A file with the GitHub token, for the user only; none: `gh auth token`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_token_file: Option<PathBuf>,
    /// `false`: the hub does not ask GitHub; none: it does when it has a token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_github: Option<bool>,
    /// The name of this PC in the tailnet (`pc.example.ts.net`), when Tailscale Serve forwards it to the hub for
    /// phones; none: a phone comes only through an SSH tunnel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailnet_name: Option<String>,
}

/// Is this a path the hub may hand to the engine? Whole (absolute), no `..`, no control character.
///
/// # Errors
///
/// The reason, naming the setting, without the path.
pub fn check_path(name: &str, path: &Path) -> Result<(), String> {
    let text = path.to_string_lossy();
    if text.is_empty() || !path.is_absolute() {
        return Err(format!("{name} must be a whole path"));
    }
    if text.chars().any(char::is_control) {
        return Err(format!("{name} holds a control character"));
    }
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!("{name} must not climb with .."));
    }
    Ok(())
}

impl AppSettings for TestingSettings {
    fn validate(&self) -> Result<(), String> {
        if let Some(name) = &self.tailnet_name
            && !crate::phone::plain_tailnet_name(name)
        {
            return Err("tailnet_name must be a plain host name like pc.example.ts.net".to_owned());
        }
        if let Some(path) = &self.state_dir {
            check_path("state_dir", path)?;
        }
        if let Some(path) = &self.areas {
            check_path("areas", path)?;
        }
        if let Some(path) = &self.github_token_file {
            check_path("github_token_file", path)?;
        }
        Ok(())
    }
}

/// Where a value came from: what the hub tells (`about`) instead of the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Env,
    Setting,
    Default,
}

impl Origin {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Env => "environment",
            Self::Setting => "setting",
            Self::Default => "default",
        }
    }
}

/// The state folder and the `areas.json` the hub hands the engine, each with where it came from:
/// the variable, then `config.json`, then the place the hub works out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    pub state_dir: Option<(PathBuf, Origin)>,
    pub areas: Option<(PathBuf, Origin)>,
    /// The `metrics.jsonl` of the heavy runs: `TESTING_METRICS_FILE` (the engine's own variable),
    /// else the place the engine writes it to.
    pub metrics: Option<(PathBuf, Origin)>,
}

/// Resolve [`Places`].
///
/// # Errors
///
/// A variable that holds a path that is not whole (a typo must not turn into results read from
/// somewhere else).
pub fn places(file: &FileConfig<TestingSettings>, env: &Env) -> Result<Places, String> {
    let pick = |var: &str,
                setting: &Option<PathBuf>,
                default: Option<PathBuf>|
     -> Result<Option<(PathBuf, Origin)>, String> {
        if let Some(path) = env.path(var) {
            check_path(var, &path)?;
            return Ok(Some((path, Origin::Env)));
        }
        if let Some(path) = setting {
            return Ok(Some((path.clone(), Origin::Setting)));
        }
        Ok(default.map(|p| (p, Origin::Default)))
    };
    Ok(Places {
        state_dir: pick(
            &crate::IDENTITY.var("STATE_DIR"),
            &file.app.state_dir,
            crate::places::nvim_state_dir(env),
        )?,
        areas: pick(
            &crate::IDENTITY.var("AREAS"),
            &file.app.areas,
            crate::places::areas_file(env),
        )?,
        metrics: if env.get("TESTING_METRICS").is_some_and(|v| v == "off") {
            None
        } else {
            pick(
                "TESTING_METRICS_FILE",
                &None,
                crate::places::metrics_file(env),
            )?
        },
    })
}

/// The `metrics.jsonl` the hub hands the engine with every `repo`: named also while it is not there
/// yet, so the first heavy run after the start shows without a restart of the hub (the engine reads
/// a missing file as no runs, the part `metrics` is then null). `None` only when there is no place
/// for it (`TESTING_METRICS=off`, no data folder).
#[must_use]
pub fn metrics_for_engine(places: &Places) -> Option<PathBuf> {
    places.metrics.as_ref().map(|(p, _)| p.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_metrics_file_is_named_before_the_first_run_writes_it() {
        let file = FileConfig::<TestingSettings>::default();
        let local = root().join("local");
        let env = Env::from_pairs([
            ("LOCALAPPDATA", local.to_str().unwrap()),
            ("XDG_DATA_HOME", local.to_str().unwrap()),
        ]);
        let got = places(&file, &env).unwrap();
        let want = local.join("testing").join("metrics.jsonl");
        assert!(!want.exists(), "a path of no file that exists");
        assert_eq!(metrics_for_engine(&got), Some(want));
        let off = Env::from_pairs([
            ("LOCALAPPDATA", local.to_str().unwrap()),
            ("TESTING_METRICS", "off"),
        ]);
        assert_eq!(metrics_for_engine(&places(&file, &off).unwrap()), None);
    }

    /// A whole path of no folder that exists: the paths are only checked, nothing is made.
    fn root() -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            "C:/testing-hub-test"
        } else {
            "/testing-hub-test"
        })
    }

    #[test]
    fn a_path_is_whole_without_climbing_or_control_characters() {
        let whole = root();
        assert!(check_path("state_dir", &whole).is_ok());
        for bad in [
            PathBuf::new(),
            PathBuf::from("relative/dir"),
            whole.join(".."),
            PathBuf::from(format!("{}\u{7}x", whole.display())),
        ] {
            let err = check_path("areas", &bad).unwrap_err();
            assert!(err.starts_with("areas "), "{bad:?}: {err}");
        }
    }

    #[test]
    fn the_settings_validate_both_paths_and_refuse_a_key_they_do_not_know() {
        let ok = TestingSettings {
            state_dir: Some(root()),
            areas: Some(root().join("areas.json")),
            github_token_file: Some(root().join("token")),
            ask_github: Some(true),
            tailnet_name: Some("pc.example.ts.net".to_owned()),
        };
        assert!(ok.validate().is_ok());
        assert!(TestingSettings::default().validate().is_ok());
        for bad in [
            TestingSettings {
                tailnet_name: Some("https://pc.example.ts.net".to_owned()),
                ..TestingSettings::default()
            },
            TestingSettings {
                state_dir: Some(PathBuf::from("nope")),
                ..TestingSettings::default()
            },
            TestingSettings {
                areas: Some(PathBuf::from("areas.json")),
                ..TestingSettings::default()
            },
            TestingSettings {
                github_token_file: Some(PathBuf::from("token")),
                ..TestingSettings::default()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        assert!(serde_json::from_str::<TestingSettings>(r#"{"state":"x"}"#).is_err());
    }

    fn file(state_dir: Option<PathBuf>, areas: Option<PathBuf>) -> FileConfig<TestingSettings> {
        FileConfig {
            app: TestingSettings {
                state_dir,
                areas,
                ..TestingSettings::default()
            },
            ..FileConfig::default()
        }
    }

    #[test]
    fn a_place_comes_from_the_variable_then_the_setting_then_the_default() {
        let tmp = root();
        let (a, b) = (tmp.join("a"), tmp.join("b"));
        let env = Env::from_pairs([
            ("TESTING_DESKTOP_STATE_DIR", a.display().to_string()),
            (
                "TASKS_DESKTOP_CONFIG_DIR",
                tmp.join("tasks").display().to_string(),
            ),
        ]);
        let got = places(&file(Some(b.clone()), Some(b.clone())), &env).unwrap();
        assert_eq!(got.state_dir, Some((a, Origin::Env)));
        assert_eq!(got.areas, Some((b.clone(), Origin::Setting)));
        let got = places(&file(None, None), &env).unwrap();
        assert_eq!(
            got.areas,
            Some((tmp.join("tasks").join("areas.json"), Origin::Default))
        );
        assert_eq!(
            places(&file(None, None), &Env::default()).unwrap().areas,
            None
        );
    }

    #[test]
    fn a_variable_with_a_path_that_is_not_whole_stops_the_start() {
        let env = Env::from_pairs([("TESTING_DESKTOP_AREAS", "relative.json")]);
        let err = places(&file(None, None), &env).unwrap_err();
        assert!(err.contains("TESTING_DESKTOP_AREAS"), "{err}");
    }
}
