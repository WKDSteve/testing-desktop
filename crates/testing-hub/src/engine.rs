//! The engine seam of the testing hub: the seam itself is `appkit_hub::engine`; what is testing.nvim's
//! own is here: its calls (`testing-export/1`, read only), its dialect, the request every call
//! carries, and how a setup becomes a `CliEngine`.
//!
//! A [`Method`] is a closed list, never a string from outside: the only way from a name to a call
//! is [`Calls::parse`]. Nothing writes.
//!
//! The engine is a child with an allow-listed environment (`appkit_hub::engine::child_env`): it
//! gets the repositories root as its "vault" (`TESTING_REPOS_ROOT`) and cannot find the state
//! folder or the data classes of the editor by itself. Both go into every request ([`Ask`]), never
//! into the environment.

pub use appkit_hub::engine::*;

use std::path::PathBuf;

use appkit_hub::discovery::{engine_script, DiscoverySpec, Setup};
use appkit_hub::env::Env;
use serde_json::{json, Map, Value};

use crate::github::{GithubToken, REMOTE_MAX_AGE_SECS};

/// The contract version this hub speaks.
pub const SCHEMA: u64 = 1;

/// How the head of every document of testing.nvim names the contract: a text, not the number.
pub const SCHEMA_TAG: &str = "testing-export/1";

/// What the read side of the app needs from the engine (`testing call hello`).
pub const REQUIRED_CAPABILITIES: &[&str] = &[
    "hello",
    "fleet",
    "repo",
    "finding",
    "params",
    "data_classes",
];

/// What the seam must know about testing.nvim.
///
/// testing.nvim writes no error document: a refusal is a line on stderr and exit 2, which the seam
/// reads as `EngineError::Failed`. It has no persistent host; [`cli_config`] is for
/// `CliEngine::new`, one process per question. It knows no extra areas.
pub const DIALECT: EngineDialect = EngineDialect {
    schema: SCHEMA,
    schema_tag: Some(SCHEMA_TAG),
    error_kind: "error",
    host_script: "testing-host.lua",
    vault_var: "TESTING_REPOS_ROOT",
    extra_areas_var: "TESTING_EXTRA_AREAS",
    required_capabilities: REQUIRED_CAPABILITIES,
    hello: Calls::Hello,
    engine_name: "the testing.nvim checkout",
};

/// What the hub looks for to start the engine: its variables, the checkout and the script in it.
pub const DISCOVERY: DiscoverySpec = DiscoverySpec {
    vault_var: "TESTING_REPOS_ROOT",
    nvim_var: "TESTING_NVIM",
    engine_dir_var: "TESTING_ENGINE_DIR",
    engine_name: "testing.nvim",
    engine_script: &["scripts", "testing.lua"],
};

/// Ask `hello` and check the answer against what this app needs.
///
/// # Errors
///
/// A [`ProbeProblem`] when the engine does not answer, speaks another contract version or lacks
/// a capability.
pub async fn probe(engine: &dyn Engine) -> Result<EngineInfo, ProbeProblem> {
    appkit_hub::engine::probe(engine, &DIALECT).await
}

/// How long one call of the engine may take. A fleet of fifty repositories asks git about each of them, and GitHub
/// with a token: this is longer than the limit of the CI adapter (`testing.ci.remote.DEADLINE_MS`, 120 s), which then
/// answers for the repositories it has no answer for. Shorter, a slow GitHub would cost the local results of the
/// whole fleet. A request of a page waits for such a call, so the limit of a request is above this one
/// (`app::limits_for`).
pub const ENGINE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(150);

/// The configuration of a `CliEngine` for what the setup found.
#[must_use]
pub fn cli_config(setup: &Setup, env: Env) -> CliEngineConfig {
    CliEngineConfig {
        dialect: DIALECT,
        nvim: setup.nvim.value.clone(),
        script: engine_script(&DISCOVERY, &setup.engine_dir.value),
        vault: setup.vault.value.clone(),
        lib_dir: setup.lib_dir.as_ref().map(|l| l.value.clone()),
        extra_areas: Vec::new(),
        env,
        timeout: ENGINE_TIMEOUT,
        max_queue: 16,
        // A document of the engine is at most 8 MiB (testing.nvim `testing.export.MAX_DOC_BYTES`).
        max_reply_bytes: 16 * 1024 * 1024,
    }
}

/// The calls of `testing-export/1` this hub makes. The seam reads the flags of each entry; it knows
/// no call by name. The kinds have no prefix: testing.nvim's documents are `fleet`, `repo`, `hello`
/// and, for `finding`, `findings` (all findings of a repository in one document).
static SPECS: [MethodSpec; 4] = [
    MethodSpec::bare("hello", "hello"),
    MethodSpec::read("fleet", "fleet"),
    MethodSpec::read("repo", "repo"),
    MethodSpec::read("finding", "findings"),
];

/// The calls of the testing engine, by name.
pub struct Calls;

#[allow(non_upper_case_globals)]
impl Calls {
    pub const Hello: Method = Method::new(&SPECS[0]);
    pub const Fleet: Method = Method::new(&SPECS[1]);
    pub const Repo: Method = Method::new(&SPECS[2]);
    pub const Finding: Method = Method::new(&SPECS[3]);

    /// Every call of the table.
    pub const ALL: [Method; 4] = [Self::Hello, Self::Fleet, Self::Repo, Self::Finding];

    /// Parse the name of a route or a request. Nothing else is accepted.
    #[must_use]
    pub fn parse(name: &str) -> Option<Method> {
        Method::find(&Self::ALL, name)
    }
}

/// What every request of the hub carries: the export form (no absolute path, only the
/// repositories the data classes release), the state folder and the `areas.json` of this device.
/// A part that is `None` is left out: the engine then says what it could not read. With a token,
/// `fleet` and `repo` also ask GitHub, within `REMOTE_MAX_AGE_SECS`; `finding` never does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ask {
    pub state_dir: Option<PathBuf>,
    pub areas: Option<PathBuf>,
    pub github_token: Option<GithubToken>,
}

impl Ask {
    /// The request of `fleet`.
    #[must_use]
    pub fn fleet(&self) -> Value {
        let mut map = self.base();
        self.ask_github(&mut map);
        Value::Object(map)
    }

    /// The request of `repo` for the repository with this key of `fleet`.
    #[must_use]
    pub fn repo(&self, key: &str) -> Value {
        let mut map = self.base();
        self.ask_github(&mut map);
        map.insert("repo".to_owned(), json!(key));
        Value::Object(map)
    }

    /// The request of `finding` for the repository with this key of `fleet`. Findings have one form
    /// and always ask the data classes, so the engine takes no `export` for them.
    #[must_use]
    pub fn finding(&self, key: &str) -> Value {
        let mut map = self.base();
        map.remove("export");
        map.insert("repo".to_owned(), json!(key));
        Value::Object(map)
    }

    fn ask_github(&self, map: &mut Map<String, Value>) {
        if let Some(token) = &self.github_token {
            map.insert("remote".to_owned(), json!(true));
            map.insert("github_token".to_owned(), json!(token.expose()));
            map.insert("remote_max_age".to_owned(), json!(REMOTE_MAX_AGE_SECS));
        }
    }

    fn base(&self) -> Map<String, Value> {
        let mut map = Map::new();
        map.insert("export".to_owned(), json!(true));
        if let Some(dir) = &self.state_dir {
            map.insert("state_dir".to_owned(), json!(dir.display().to_string()));
        }
        if let Some(file) = &self.areas {
            map.insert("areas".to_owned(), json!(file.display().to_string()));
        }
        map
    }
}

/// Is this a key of `fleet` (`<name>-<12 hex>`, testing.nvim `testing.export.repo_key`): nothing
/// that could name a path, a query or an option.
#[must_use]
pub fn is_repo_key(key: &str) -> bool {
    let Some((name, hash)) = key.rsplit_once('-') else {
        return false;
    };
    key.len() <= 140
        && !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        && name != "."
        && name != ".."
        && hash.len() == 12
        && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use appkit_hub::config::{Resolved, Source};

    use super::*;

    #[test]
    fn nothing_writes_hello_takes_nothing_and_none_runs_aside() {
        for m in Calls::ALL {
            assert!(!m.writes(), "{}", m.as_str());
            assert!(!m.is_aside(), "{}", m.as_str());
        }
        let bare: Vec<_> = Calls::ALL
            .iter()
            .filter(|m| !m.takes_params())
            .map(|m| m.as_str())
            .collect();
        assert_eq!(bare, ["hello"]);
    }

    #[test]
    fn every_name_round_trips_and_every_kind_is_testing_nvims() {
        for m in Calls::ALL {
            assert_eq!(Calls::parse(m.as_str()), Some(m));
        }
        assert_eq!(Calls::Hello.reply_kind(), "hello");
        assert_eq!(Calls::Fleet.reply_kind(), "fleet");
        assert_eq!(Calls::Repo.reply_kind(), "repo");
        assert_eq!(Calls::Finding.reply_kind(), "findings");
        for bad in ["", "Fleet", "testing.fleet", "findings", "export", "ops"] {
            assert_eq!(Calls::parse(bad), None, "{bad:?}");
        }
    }

    /// These names are in the contract of testing.nvim and in the environments of people.
    #[test]
    fn the_dialect_names_the_testing_engine() {
        assert_eq!(DIALECT.schema, 1);
        assert_eq!(DIALECT.schema_tag, Some("testing-export/1"));
        assert_eq!(DIALECT.vault_var, "TESTING_REPOS_ROOT");
        assert_eq!(DIALECT.hello, Calls::Hello);
        assert_eq!(DISCOVERY.vault_var, DIALECT.vault_var);
        assert!(DIALECT.head_schema_matches(Some(&json!("testing-export/1"))));
        assert!(!DIALECT.head_schema_matches(Some(&json!(1))));
        // Every capability the hub needs is one testing.nvim's hello names (testing.export.HELLO_CAPABILITIES).
        for needed in REQUIRED_CAPABILITIES {
            assert!(
                [
                    "data_classes",
                    "finding",
                    "fleet",
                    "hello",
                    "manifest",
                    "params",
                    "repo"
                ]
                .contains(needed),
                "{needed}"
            );
        }
    }

    fn resolved(path: &str) -> Resolved<PathBuf> {
        Resolved {
            value: PathBuf::from(path),
            source: Source::Setting,
        }
    }

    #[test]
    fn the_engine_is_started_with_the_root_as_its_vault_and_the_script_of_the_checkout() {
        let setup = Setup {
            nvim: resolved("/opt/nvim/bin/nvim"),
            nvim_version: (0, 12, 2),
            engine_dir: resolved("/src/testing.nvim"),
            lib_dir: Some(resolved("/src/lib.nvim")),
            lib_problem: None,
            vault: resolved("/repos"),
            extra_areas: vec!["/elsewhere".to_owned()],
        };
        let cfg = cli_config(&setup, Env::from_pairs::<_, &str, &str>([]));
        assert_eq!(
            cfg.script,
            Path::new("/src/testing.nvim")
                .join("scripts")
                .join("testing.lua")
        );
        assert_eq!(cfg.vault, Path::new("/repos"));
        assert_eq!(cfg.lib_dir.as_deref(), Some(Path::new("/src/lib.nvim")));
        assert!(cfg.extra_areas.is_empty(), "{:?}", cfg.extra_areas);
    }

    #[test]
    fn every_request_is_the_export_form_with_what_is_known() {
        let none = Ask::default();
        assert_eq!(none.fleet(), json!({ "export": true }));
        let ask = Ask {
            state_dir: Some(PathBuf::from("/state/nvim")),
            areas: Some(PathBuf::from("/config/tasks-desktop/areas.json")),
            github_token: None,
        };
        assert_eq!(
            ask.fleet(),
            json!({
                "export": true,
                "state_dir": "/state/nvim",
                "areas": "/config/tasks-desktop/areas.json",
            })
        );
        assert_eq!(
            ask.repo("alpha.nvim-0123456789ab")["repo"],
            "alpha.nvim-0123456789ab"
        );
        assert_eq!(ask.repo("alpha.nvim-0123456789ab")["export"], true);
        // The engine refuses `export` for findings: they have one form.
        assert_eq!(
            ask.finding("alpha.nvim-0123456789ab"),
            json!({
                "repo": "alpha.nvim-0123456789ab",
                "state_dir": "/state/nvim",
                "areas": "/config/tasks-desktop/areas.json",
            })
        );
    }

    #[test]
    fn a_token_goes_into_the_requests_that_ask_github_and_no_other() {
        const TOKEN: &str = "github_pat_0123456789abcdefABCDEF";
        let ask = Ask {
            github_token: GithubToken::parse(TOKEN),
            ..Ask::default()
        };
        let key = "alpha.nvim-0123456789ab";
        for request in [ask.fleet(), ask.repo(key)] {
            assert_eq!(request["remote"], true);
            assert_eq!(request["github_token"], TOKEN);
            assert_eq!(request["remote_max_age"], REMOTE_MAX_AGE_SECS);
        }
        let finding = ask.finding(key);
        assert!(!finding.to_string().contains(TOKEN), "{finding}");
        assert!(finding.get("remote").is_none(), "{finding}");
        // Without a token the hub does not ask.
        assert!(Ask::default().fleet().get("remote").is_none());
        // And the request is what stands in `Debug` of the settings: the token does not.
        assert!(!format!("{ask:?}").contains(TOKEN));
    }

    #[test]
    fn a_key_is_a_name_and_twelve_hex_and_nothing_else() {
        for ok in [
            "alpha.nvim-0123456789ab",
            "lib.nvim-ABCDEF012345",
            "my_repo-ffffffffffff",
            "a-b-c-000000000000",
        ] {
            assert!(is_repo_key(ok), "{ok}");
        }
        for bad in [
            "",
            "alpha.nvim",
            "alpha.nvim-0123",
            "alpha.nvim-0123456789abc",
            "alpha.nvim-0123456789ag",
            "-0123456789ab",
            "..-0123456789ab",
            "a/b-0123456789ab",
            "a\\b-0123456789ab",
            "a b-0123456789ab",
            "a%2e-0123456789ab",
            "a&x=1-0123456789ab",
        ] {
            assert!(!is_repo_key(bad), "{bad:?}");
        }
        assert!(!is_repo_key(&format!("{}-0123456789ab", "a".repeat(140))));
    }
}
