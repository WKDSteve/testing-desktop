//! The settings of this app in `config.json`, beside the keys of the kit (vault, nvim, engine_dir, port).
//!
//! `state_dir` is the state folder of testing.nvim the hub reads the results from (`stdpath('state')` of
//! the editor that runs the tests). The engine is a child with an allow-listed environment, so it cannot
//! find that folder by itself; the hub hands it over as a parameter of every call, never through the
//! environment. It is a statement of the person about this device, never of the page.
//! `TESTING_DESKTOP_STATE_DIR` overrides the file.

use std::path::{Path, PathBuf};

use appkit_hub::config::AppSettings;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TestingSettings {
    /// The state folder of testing.nvim; none: the hub asks the engine without one and says so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_dir: Option<PathBuf>,
}

/// Is this a path the hub may hand to the engine as the state folder? Whole (absolute), no `..`, no
/// control character.
///
/// # Errors
///
/// The reason, without the path.
pub fn check_state_dir(path: &Path) -> Result<(), &'static str> {
    let text = path.to_string_lossy();
    if text.is_empty() || !path.is_absolute() {
        return Err("state_dir must be a whole path");
    }
    if text.chars().any(char::is_control) {
        return Err("state_dir holds a control character");
    }
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("state_dir must not climb with ..");
    }
    Ok(())
}

impl AppSettings for TestingSettings {
    fn validate(&self) -> Result<(), String> {
        match &self.state_dir {
            Some(path) => check_state_dir(path).map_err(str::to_owned),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_dir_is_a_whole_path_without_climbing_or_control_characters() {
        let whole = std::env::temp_dir();
        assert!(check_state_dir(&whole).is_ok());
        for bad in [
            PathBuf::new(),
            PathBuf::from("relative/dir"),
            whole.join(".."),
            PathBuf::from(format!("{}\u{7}x", whole.display())),
        ] {
            assert!(check_state_dir(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_settings_validate_their_state_dir_and_refuse_a_key_they_do_not_know() {
        let ok = TestingSettings {
            state_dir: Some(std::env::temp_dir()),
        };
        assert!(ok.validate().is_ok());
        assert!(TestingSettings::default().validate().is_ok());
        let bad = TestingSettings {
            state_dir: Some(PathBuf::from("nope")),
        };
        assert!(bad.validate().is_err());
        assert!(serde_json::from_str::<TestingSettings>(r#"{"state":"x"}"#).is_err());
    }
}
