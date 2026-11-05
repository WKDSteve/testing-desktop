//! The token the hub asks GitHub with, through the engine. The child of the engine has neither the login of `gh` nor a
//! token in its environment (the allow-list of the kit), so the hub holds one and hands it over in the requests of
//! `fleet` and `repo`, as JSON on stdin of the child: never in an argument, never in the environment.
//!
//! It comes from `github_token_file` of `config.json` (a statement of the person: when it cannot be used the start
//! stops), else from one `gh auth token` at the start, without a shell. No answer of the hub holds it, and
//! [`GithubToken`] has no way to print it.

use std::ffi::OsStr;
use std::fmt;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use appkit_hub::HubStatus;

use crate::settings::TestingSettings;

/// How old the answers of GitHub may be, in seconds (`remote_max_age`): however often the fleet is read, a page opened or
/// a test run, the engine asks GitHub at most once in this time per repository.
pub const REMOTE_MAX_AGE_SECS: u64 = 60;

/// What `hello` of an engine says when it takes a token in the request.
pub const REMOTE_CAPABILITY: &str = "remote_token";

/// The longest `gh auth token` may take.
const GH_TIMEOUT: Duration = Duration::from_secs(10);
/// A token file is a token: more than this is another file.
const MAX_FILE_BYTES: u64 = 4096;
/// The length of a token, as testing.nvim takes it (`testing.export.call`).
const TOKEN_LEN: std::ops::RangeInclusive<usize> = 20..=512;

/// A GitHub token. Not printable: no `Display`, no `Serialize`, and `Debug` says only that it is one.
#[derive(Clone, PartialEq, Eq)]
pub struct GithubToken(String);

impl fmt::Debug for GithubToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GithubToken(<redacted>)")
    }
}

impl GithubToken {
    /// The token in this text, with the white space around it; `None`: it is not one.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        (TOKEN_LEN.contains(&text.len())
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')))
        .then(|| Self(text.to_owned()))
    }

    /// The text, for the request to the engine and nothing else.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

/// Whether the hub asks GitHub, and why not: what `about` says, as a word.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CiState {
    /// With the token of `github_token_file`.
    File,
    /// With the token of `gh auth token`.
    Gh,
    /// No file, and `gh` gave none.
    #[default]
    NoToken,
    /// The engine of this machine takes no token in the request.
    EngineTooOld,
    /// The hub has no engine.
    NoEngine,
    /// `ask_github = false` in `config.json`.
    Off,
}

impl CiState {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Gh => "gh",
            Self::NoToken => "no_token",
            Self::EngineTooOld => "engine_too_old",
            Self::NoEngine => "no_engine",
            Self::Off => "off",
        }
    }
}

/// The token of a file. The texts name the setting, never the path or a line of the file.
///
/// # Errors
///
/// The file cannot be read, others can read it (not on Windows, where the folder decides), or it holds no token.
pub fn from_file(path: &Path) -> Result<GithubToken, String> {
    let unreadable = |_| "github_token_file cannot be read".to_owned();
    let meta = std::fs::metadata(path).map_err(unreadable)?;
    only_for_the_user(&meta)?;
    if meta.len() > MAX_FILE_BYTES {
        return Err("github_token_file is too large to be a token".to_owned());
    }
    let text = std::fs::read_to_string(path).map_err(unreadable)?;
    GithubToken::parse(&text).ok_or_else(|| "github_token_file holds no GitHub token".to_owned())
}

#[cfg(unix)]
fn only_for_the_user(meta: &std::fs::Metadata) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if meta.permissions().mode() & 0o077 == 0 {
        Ok(())
    } else {
        Err("github_token_file can be read by others (chmod 600 it)".to_owned())
    }
}

#[cfg(not(unix))]
fn only_for_the_user(_meta: &std::fs::Metadata) -> Result<(), String> {
    Ok(())
}

/// One `<program> auth token`: arguments, no shell, a time limit. `None` when it gives no token, whatever the reason.
pub async fn ask_program(program: &OsStr) -> Option<GithubToken> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(["auth", "token"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    // CREATE_NO_WINDOW: no console window for a console program.
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let output = tokio::time::timeout(GH_TIMEOUT, command.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    GithubToken::parse(std::str::from_utf8(&output.stdout).ok()?)
}

/// The token of `gh`, the way a person is logged in on this machine.
pub async fn from_gh() -> Option<GithubToken> {
    ask_program(OsStr::new("gh")).await
}

/// The token the hub holds, and what `about` says of it. `gh` is only asked when its token can be used.
///
/// # Errors
///
/// A `github_token_file` that cannot be used stops the start: a typo must not turn into a hub that quietly does not ask.
pub async fn resolve(
    settings: &TestingSettings,
    status: &HubStatus,
    gh: impl AsyncFnOnce() -> Option<GithubToken>,
) -> Result<(Option<GithubToken>, CiState), String> {
    if settings.ask_github == Some(false) {
        return Ok((None, CiState::Off));
    }
    let file = settings
        .github_token_file
        .as_deref()
        .map(from_file)
        .transpose()?;
    if !status.engine_ready {
        return Ok((None, CiState::NoEngine));
    }
    if !status.capabilities.iter().any(|c| c == REMOTE_CAPABILITY) {
        return Ok((None, CiState::EngineTooOld));
    }
    if let Some(token) = file {
        return Ok((Some(token), CiState::File));
    }
    Ok(match gh().await {
        Some(token) => (Some(token), CiState::Gh),
        None => (None, CiState::NoToken),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const TOKEN: &str = "github_pat_0123456789abcdefABCDEF";

    fn status(ready: bool, capabilities: &[&str]) -> HubStatus {
        HubStatus {
            engine_ready: ready,
            problem: None,
            capabilities: capabilities.iter().map(|c| (*c).to_owned()).collect(),
        }
    }

    #[test]
    fn a_token_is_a_plain_word_of_a_length_and_shows_in_no_debug_output() {
        assert_eq!(
            GithubToken::parse(&format!("  {TOKEN}\r\n"))
                .unwrap()
                .expose(),
            TOKEN
        );
        for bad in [
            "",
            "short",
            "with a space in it, long enough",
            "gho_line\nbreak_long_enough_to_pass",
            &"a".repeat(513),
        ] {
            assert!(GithubToken::parse(bad).is_none(), "{bad:?}");
        }
        let debug = format!("{:?}", GithubToken::parse(TOKEN).unwrap());
        assert!(!debug.contains(TOKEN), "{debug}");
    }

    #[test]
    fn every_state_has_its_own_word() {
        let words = [
            CiState::File,
            CiState::Gh,
            CiState::NoToken,
            CiState::EngineTooOld,
            CiState::NoEngine,
            CiState::Off,
        ]
        .map(CiState::word);
        let mut unique = words.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), words.len());
    }

    fn with_file<T>(name: &str, text: &[u8], check: impl FnOnce(&Path) -> T) -> T {
        let dir = appkit_scratch::Scratch::new(name);
        let path = dir.join("token");
        std::fs::write(&path, text).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        check(&path)
    }

    #[test]
    fn a_file_holds_the_token_and_a_refusal_quotes_neither_path_nor_content() {
        let good = with_file("gh-good", format!("{TOKEN}\n").as_bytes(), from_file);
        assert_eq!(good.unwrap().expose(), TOKEN);

        let other = "this is no token, but a secret line";
        let err = with_file("gh-bad", other.as_bytes(), from_file).unwrap_err();
        assert!(err.starts_with("github_token_file"), "{err}");
        assert!(!err.contains("secret"), "{err}");

        let large = vec![b'a'; 5000];
        assert!(with_file("gh-large", &large, from_file).is_err());

        let missing = Path::new(if cfg!(windows) {
            "C:/td/nope"
        } else {
            "/td/nope"
        });
        let err = from_file(missing).unwrap_err();
        assert!(!err.contains("nope"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_file_others_can_read_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        with_file("gh-open", TOKEN.as_bytes(), |path| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(from_file(path).unwrap_err().contains("others"));
        });
    }

    #[tokio::test]
    async fn a_program_that_is_not_there_gives_no_token() {
        assert!(ask_program(OsStr::new("no-such-program-of-testing-hub"))
            .await
            .is_none());
    }

    #[tokio::test]
    async fn the_file_comes_first_and_gh_is_asked_only_when_its_token_can_be_used() {
        let asked = Cell::new(0);
        let gh = async || {
            asked.set(asked.get() + 1);
            GithubToken::parse(TOKEN)
        };
        let ready = status(true, &["hello", REMOTE_CAPABILITY]);
        let plain = TestingSettings::default();

        let (token, ci) = resolve(&plain, &ready, gh).await.unwrap();
        assert_eq!((token.is_some(), ci), (true, CiState::Gh));
        assert_eq!(asked.get(), 1);

        let (token, ci) = resolve(&plain, &ready, async || None).await.unwrap();
        assert_eq!((token, ci), (None, CiState::NoToken));

        for (settings, status, want) in [
            (
                TestingSettings {
                    ask_github: Some(false),
                    ..TestingSettings::default()
                },
                ready.clone(),
                CiState::Off,
            ),
            (
                plain.clone(),
                status(true, &["hello"]),
                CiState::EngineTooOld,
            ),
            (plain.clone(), status(false, &[]), CiState::NoEngine),
        ] {
            let gh = async || {
                asked.set(asked.get() + 1);
                GithubToken::parse(TOKEN)
            };
            let (token, ci) = resolve(&settings, &status, gh).await.unwrap();
            assert_eq!((token, ci), (None, want));
        }
        assert_eq!(asked.get(), 1, "gh is not asked for a token nobody can use");
    }

    #[tokio::test]
    async fn a_token_file_is_taken_before_gh_and_one_that_is_broken_stops_the_start() {
        let dir = appkit_scratch::Scratch::new("gh-resolve");
        let path = dir.join("token");
        std::fs::write(&path, TOKEN).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let settings = TestingSettings {
            github_token_file: Some(path.clone()),
            ..TestingSettings::default()
        };
        let ready = status(true, &[REMOTE_CAPABILITY]);
        let (token, ci) = resolve(&settings, &ready, async || panic!("gh must not be asked"))
            .await
            .unwrap();
        assert_eq!(
            (token.unwrap().expose().to_owned(), ci),
            (TOKEN.to_owned(), CiState::File)
        );

        std::fs::write(&path, "no token").unwrap();
        assert!(resolve(&settings, &ready, async || None).await.is_err());
        // Also with an engine that could not use it: the setting is wrong either way.
        assert!(resolve(&settings, &status(false, &[]), async || None)
            .await
            .is_err());
    }
}
