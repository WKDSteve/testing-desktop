//! The program as a person runs it: arguments in, text and exit status out. Each run gets a config
//! folder of its own, and no variable of the person who runs the test that belongs to one of the
//! hubs reaches it.

use std::path::Path;
use std::process::{Command, Output};

use appkit_scratch::Scratch;

fn run(config: &Path, args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_testing-hub"));
    c.args(args).env_clear();
    for (k, v) in std::env::vars_os() {
        let name = k.to_string_lossy().to_ascii_uppercase();
        if !["TESTING_", "RULES_", "TASKS_", "HUB_"]
            .iter()
            .any(|p| name.starts_with(p))
        {
            c.env(k, v);
        }
    }
    c.env("TESTING_DESKTOP_CONFIG_DIR", config)
        .output()
        .expect("the program starts")
}

#[test]
fn version_prints_one_line_with_the_name_and_the_version() {
    let config = Scratch::new("cli-version");
    let out = run(&config, &["--version"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.trim_end(), testing_hub::version_line());
    assert!(text.starts_with("testing-hub "), "{text}");
}

#[test]
fn help_names_every_verb() {
    let config = Scratch::new("cli-help");
    let out = run(&config, &["--help"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    for verb in [
        "serve",
        "start",
        "stop",
        "autostart",
        "install",
        "uninstall",
        "open",
        "--version",
    ] {
        assert!(text.contains(&format!("testing-hub {verb}")), "{verb}");
    }
}

#[test]
fn an_unknown_verb_or_a_bad_port_is_refused_with_the_usage() {
    let config = Scratch::new("cli-bad");
    for args in [
        &["frobnicate"][..],
        &["serve", "--port", "80"],
        &["serve", "--port", "x"],
        &["stop", "now"],
        &["autostart", "maybe"],
        &["install", "--everything"],
    ] {
        let out = run(&config, args);
        assert_eq!(out.status.code(), Some(1), "{args:?}: {out:?}");
        let err = String::from_utf8(out.stderr).unwrap();
        assert!(err.starts_with("testing-hub: "), "{args:?}: {err}");
    }
}

#[test]
fn open_without_a_running_hub_says_so() {
    let config = Scratch::new("cli-open");
    let out = run(&config, &["open"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("no hub is running"), "{err}");
}

#[test]
fn a_config_with_a_bad_state_dir_stops_the_start() {
    let config = Scratch::new("cli-state-dir");
    std::fs::write(
        config.join("config.json"),
        r#"{"state_dir":"relative/state"}"#,
    )
    .unwrap();
    let out = run(&config, &["serve", "--port", "17698"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("state_dir"), "{err}");
}
