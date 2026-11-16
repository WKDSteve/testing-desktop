//! A hub in the background ends itself (appkit `background::left_alone`): the real program, `start` with a config
//! folder of its own, a short time alone (`TESTING_DESKTOP_ALONE_SECS`), no engine (a Neovim that is not there, so
//! the hub starts at once and asks nobody), and the login entries of the machine left out
//! (`TESTING_DESKTOP_NO_LOGIN_SYNC`).

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use appkit_scratch::Scratch;
use appkit_testkit::wire::{parse, request, Reply};
use serde_json::Value;

fn command(config: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_testing-hub"));
    c.env_clear();
    for (k, v) in std::env::vars_os() {
        let name = k.to_string_lossy().to_ascii_uppercase();
        if !["TESTING_", "RULES_", "TASKS_", "HUB_", "LIB_NVIM"]
            .iter()
            .any(|p| name.starts_with(p))
        {
            c.env(k, v);
        }
    }
    c.env("TESTING_DESKTOP_CONFIG_DIR", config)
        .env("TESTING_DESKTOP_NO_LOGIN_SYNC", "1");
    c
}

/// A config folder whose hub has no engine and asks GitHub nothing, with `extra` keys of `config.json`.
fn config(name: &str, extra: &str) -> Scratch {
    let dir = Scratch::new(&format!("background-{name}"));
    let nvim = dir.join("no-nvim.exe");
    let file = format!(
        r#"{{"ask_github":false,"nvim":{}{extra}}}"#,
        serde_json::to_string(&nvim.display().to_string()).unwrap()
    );
    std::fs::write(dir.join("config.json"), file).unwrap();
    dir
}

/// A port nobody uses right now (best effort: it is released again).
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// Stops the hub of a config folder when the test ends, however it ends.
struct Background<'a>(&'a Path);

impl Drop for Background<'_> {
    fn drop(&mut self) {
        let _ = command(self.0).arg("stop").output();
    }
}

/// `start` on a port of its own with a short time alone.
fn start_in_background(config: &Path, port: u16, alone_secs: u32) {
    let started = command(config)
        .env("TESTING_DESKTOP_PORT", port.to_string())
        .env("TESTING_DESKTOP_ALONE_SECS", alone_secs.to_string())
        .arg("start")
        .output()
        .unwrap();
    assert!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stderr)
    );
    assert!(
        String::from_utf8_lossy(&started.stdout).contains("runs in the background"),
        "{started:?}"
    );
}

/// Whether a hub answers on the port.
fn answers(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok()
}

/// Wait until the hub of this config folder is gone: no record, nothing on the port.
fn gone_within(config: &Path, port: u16, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if !config.join("hub.json").exists() && !answers(port) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn host(port: u16) -> String {
    format!("127.0.0.1:{port}")
}

/// One request, answered whole.
fn send(port: u16, bytes: &[u8]) -> Reply {
    let mut stream = std::net::TcpStream::connect(host(port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    stream.write_all(bytes).unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    parse(&buf)
}

/// One answer on a connection that stays open: the head, then as many bytes as `Content-Length` says.
fn read_one(stream: &mut std::net::TcpStream) -> Reply {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        buf.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&buf).to_ascii_lowercase();
    let len = head
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .expect("a length");
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).unwrap();
    buf.extend_from_slice(&body);
    parse(&buf)
}

fn hello(port: u16) -> Value {
    let host = host(port);
    let r = send(
        port,
        &request(
            "GET",
            &testing_hub::IDENTITY.route("hello"),
            &[("Host", &host)],
            b"",
        ),
    );
    assert_eq!(r.status, 200, "{r:?}");
    r.json()
}

/// A hub that was started for a page nobody opened (a browser that never came) does not stay behind: it has no
/// window, so nobody would ever see or stop it.
#[test]
fn a_hub_in_the_background_that_nobody_opens_ends_itself() {
    let config = config("alone", "");
    let port = free_port();
    let _guard = Background(&config);
    // Ten seconds, not two: `start` has to see the hub answer while it lives, also on a machine under load.
    start_in_background(&config, port, 10);

    assert!(
        gone_within(&config, port, Duration::from_secs(40)),
        "the hub still runs: {}",
        std::fs::read_to_string(config.join("hub.log")).unwrap_or_default()
    );
    // It left on its own terms: its record and its secret are gone, and it said why in its log.
    assert!(!config.join("control.secret").exists());
    let log = std::fs::read_to_string(config.join("hub.log")).unwrap();
    assert!(log.contains("no page is connected"), "{log}");
    let none = command(&config).arg("stop").output().unwrap();
    assert!(String::from_utf8_lossy(&none.stdout).contains("no hub is running"));
}

/// The hub stays as long as a page holds a connection with its session, and goes once that page is closed.
#[test]
fn a_hub_in_the_background_stays_while_a_page_is_connected_and_ends_after_it_closed() {
    let config = config("page", "");
    let port = free_port();
    let _guard = Background(&config);
    let alone: u32 = 8;
    start_in_background(&config, port, alone);

    // The page: a pairing link, its session, and a connection that stays open (the event stream).
    let out = command(&config).arg("open").output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let url = String::from_utf8(out.stdout).unwrap();
    let code = url
        .lines()
        .next()
        .unwrap()
        .trim()
        .strip_prefix(&format!("http://127.0.0.1:{port}/#pair="))
        .expect("a pairing link")
        .to_owned();
    let host = host(port);
    let origin = format!("http://{host}");
    let body = format!(r#"{{"code":"{code}"}}"#);
    let len = body.len().to_string();
    let paired = send(
        port,
        &request(
            "POST",
            "/auth/exchange",
            &[
                ("Host", &host),
                ("Origin", &origin),
                ("Content-Type", "application/json"),
                ("Content-Length", &len),
            ],
            body.as_bytes(),
        ),
    );
    assert_eq!(paired.status, 200, "{paired:?}");
    let token = paired.json()["token"].as_str().unwrap().to_owned();
    // A page that asks the hub every few seconds on one connection that stays open, as a page does (without an
    // engine the event stream is refused, so the page asks the state of the run).
    let mut page = std::net::TcpStream::connect(&host).unwrap();
    page.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let ask = |page: &mut std::net::TcpStream| {
        write!(
            page,
            "GET {} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {token}\r\n\r\n",
            testing_hub::IDENTITY.route("run")
        )
        .unwrap();
        let reply = read_one(page);
        assert_eq!(reply.status, 200, "{reply:?}");
    };
    // More than twice the time alone: the page is still connected, so the hub is still there.
    let until = Instant::now() + Duration::from_secs(u64::from(alone) * 2 + 4);
    while Instant::now() < until {
        ask(&mut page);
        std::thread::sleep(Duration::from_secs(2));
    }
    assert_eq!(hello(port)["app"], "testing-desktop");
    assert!(config.join("hub.json").exists());

    drop(page);
    assert!(
        gone_within(&config, port, Duration::from_secs(u64::from(alone) + 40)),
        "the page is closed and the hub still runs"
    );
}

/// An order to stay is the person's: `keep_running` in the settings keeps a hub in the background without a page.
/// A hub in a terminal is never left alone either: the person there ends it.
#[test]
fn a_hub_with_an_order_to_stay_and_a_hub_in_a_terminal_do_not_end_themselves() {
    let config = config("keep", r#","keep_running":true"#);
    let port = free_port();
    let _guard = Background(&config);
    start_in_background(&config, port, 1);

    let terminal = self::config("terminal", "");
    let terminal_port = free_port();
    let child = command(&terminal)
        .env("TESTING_DESKTOP_ALONE_SECS", "1")
        .args(["serve", "--port", &terminal_port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("testing-hub starts");
    let _hub = Terminal(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !terminal.join("hub.json").exists() {
        assert!(Instant::now() < deadline, "the hub never wrote hub.json");
        std::thread::sleep(Duration::from_millis(50));
    }

    std::thread::sleep(Duration::from_secs(6));
    assert_eq!(hello(port)["app"], "testing-desktop");
    assert_eq!(hello(terminal_port)["app"], "testing-desktop");
    // `stop` ends the hub of the terminal too, in an orderly way: it takes its record with it.
    let stopped = command(&terminal).arg("stop").output().unwrap();
    assert!(stopped.status.success(), "{stopped:?}");
    assert!(
        gone_within(&terminal, terminal_port, Duration::from_secs(20)),
        "the hub of the terminal still runs"
    );
}

/// The hub of a terminal, ended when the test ends.
struct Terminal(Child);

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
