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
        "restart",
        "status",
        "open",
        "link",
        "logs",
        "paths",
        "autostart",
        "install",
        "uninstall",
        "phone pair",
        "phone list",
        "phone level",
        "phone revoke",
        "version",
    ] {
        assert!(text.contains(&format!("testing-desktop {verb}")), "{verb}");
    }
    assert!(text.contains("Exit codes"), "{text}");
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
        &["close"],
        &["logs", "-n", "0"],
        &["phone"],
        &["phone", "frobnicate"],
        &["help", "frobnicate"],
    ] {
        let out = run(&config, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        let err = String::from_utf8(out.stderr).unwrap();
        assert!(err.starts_with("testing-hub: "), "{args:?}: {err}");
    }
}

#[test]
fn link_without_a_running_hub_says_so() {
    let config = Scratch::new("cli-link");
    let out = run(&config, &["link"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("no hub is running"), "{err}");
    assert!(err.contains("testing-desktop open"), "{err}");
}

/// `status` of a hub that does not run: its words, JSON, and exit code 1; `quit` and `stop` with nothing running
/// succeed; `paths` names the settings folder of the run.
#[test]
fn status_without_a_hub_is_exit_code_1_and_the_other_common_commands_answer() {
    let config = Scratch::new("cli-status");
    let off = run(&config, &["status"]);
    assert_eq!(off.status.code(), Some(1), "{off:?}");
    assert!(String::from_utf8_lossy(&off.stdout).starts_with("testing-desktop: not running\n"));
    let json = run(&config, &["status", "--json"]);
    assert_eq!(json.status.code(), Some(1), "{json:?}");
    let doc: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(doc["running"], false);
    assert_eq!(doc["app"], "testing-desktop");
    for word in ["stop", "quit"] {
        let out = run(&config, &[word]);
        assert!(out.status.success(), "{word}: {out:?}");
        assert!(String::from_utf8_lossy(&out.stdout).contains("no hub is running"));
    }
    let paths = run(&config, &["paths"]);
    assert!(paths.status.success(), "{paths:?}");
    assert!(
        String::from_utf8_lossy(&paths.stdout)
            .contains(&format!("settings:  {}", config.display())),
        "{paths:?}"
    );
    let logs = run(&config, &["logs"]);
    assert_eq!(logs.status.code(), Some(1), "{logs:?}");
    assert!(String::from_utf8_lossy(&logs.stderr).contains("no log at"));
}

/// `help <command>` and `<command> --help`: a common command, an own one (`phone`) and `open`.
#[test]
fn help_of_one_command_is_its_own_text() {
    let config = Scratch::new("cli-help-one");
    let said = |args: &[&str]| {
        let out = run(&config, args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap()
    };
    assert!(said(&["help", "status"]).starts_with("testing-desktop status [--json]"));
    assert!(said(&["link", "--help"]).starts_with("testing-desktop link"));
    let phone = said(&["phone", "--help"]);
    assert!(
        phone.contains("testing-desktop phone pair") && phone.contains("phone revoke <id>"),
        "{phone}"
    );
    assert!(said(&["help", "install"]).contains("make the"));
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
