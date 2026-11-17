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
    assert!(err.contains("testing-hub start --open"), "{err}");
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

/// On Linux and macOS the program is also the watcher of the process group of each child it starts (appkit's
/// `child::watcher_main`, the first call of `main`): started with `--appkit-child-watcher` it only waits on descriptor
/// 3, here a pipe with one byte in it (the group released), and ends without a word. Without that call the argument
/// would be a usage error. In a process group of its own, so nothing of the test run is ever its group.
#[cfg(unix)]
#[test]
fn the_program_is_the_watcher_of_its_childrens_groups() {
    use std::io::Write;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    let mut watcher = Command::new("/bin/sh")
        .args(["-c", "exec \"$0\" --appkit-child-watcher 3<&0"])
        .arg(env!("CARGO_BIN_EXE_testing-hub"))
        .env_clear()
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the program starts");
    let mut pipe = watcher.stdin.take().unwrap();
    pipe.write_all(&[1]).unwrap();
    drop(pipe);
    let out = watcher.wait_with_output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
}
