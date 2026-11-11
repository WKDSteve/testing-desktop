//! `POST /api/testing/ci-rerun`: start the failed jobs of a red CI run again, the one thing a phone may start
//! (`phone::DEVICE_ROUTES`, level `trigger`, confirmed on the phone per request).
//!
//! The request names a repository of the fleet by its key, nothing else. The hub looks the run up in the fleet it
//! holds: the latest run of the CI workflow for the head of origin (`ci_remote`), and only when it failed, was
//! cancelled or found no runner. A run the runner guard refused (`rejected`) is not started again: that needs
//! `testing ci release` on the PC, where the person looks at the commit. The hub then runs GitHub's own command,
//! `gh run rerun <id> --failed -R <owner>/<repo>`, as an argv with no shell, its input closed, under a time limit.
//! Nothing of the request reaches the command but the key, and the key only chooses a row of the fleet.

use std::ffi::OsString;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use appkit_hub::{error_body, BoxFuture};
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::engine::is_repo_key;
use crate::github::GithubToken;
use crate::routes::Shared;

/// The states of a run that may be started again from the phone.
pub const RERUNNABLE: [&str; 3] = ["failure", "cancelled", "no_runner"];
/// How long `gh` may take.
const GH_TIMEOUT: Duration = Duration::from_secs(30);

/// Starts the failed jobs of a run again. The program's is [`GhRerun`]; a test brings its own.
pub trait Rerunner: Send + Sync {
    /// Rerun the failed jobs of run `id` of `repo` (`owner/name`).
    fn rerun(&self, repo: &str, id: u64) -> BoxFuture<'_, Result<(), String>>;
}

/// The arguments of `gh` for a rerun: a fixed command, the id and the repository, nothing else.
#[must_use]
pub fn gh_args(repo: &str, id: u64) -> Vec<String> {
    vec![
        "run".to_owned(),
        "rerun".to_owned(),
        id.to_string(),
        "--failed".to_owned(),
        "-R".to_owned(),
        repo.to_owned(),
    ]
}

/// Is `repo` `owner/name` of plain characters (what GitHub allows in both)?
#[must_use]
pub fn plain_repo(repo: &str) -> bool {
    let plain = |part: &str| {
        (1..=100).contains(&part.len())
            && !part.starts_with('-')
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            && part != "."
            && part != ".."
    };
    repo.split_once('/')
        .is_some_and(|(owner, name)| plain(owner) && plain(name))
}

/// `gh`, with the token of the hub when it has one from a file (`GH_TOKEN`), else with gh's own login.
pub struct GhRerun {
    pub program: OsString,
    pub token: Option<GithubToken>,
}

impl Rerunner for GhRerun {
    fn rerun(&self, repo: &str, id: u64) -> BoxFuture<'_, Result<(), String>> {
        let repo = repo.to_owned();
        Box::pin(async move {
            let mut command = tokio::process::Command::new(&self.program);
            command
                .args(gh_args(&repo, id))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            if let Some(token) = &self.token {
                command.env("GH_TOKEN", token.expose());
            }
            let child = command
                .spawn()
                .map_err(|_| "gh could not be started".to_owned())?;
            match tokio::time::timeout(GH_TIMEOUT, child.wait_with_output()).await {
                Ok(Ok(out)) if out.status.success() => Ok(()),
                Ok(Ok(_)) => Err("gh refused the rerun".to_owned()),
                Ok(Err(_)) => Err("gh failed".to_owned()),
                Err(_) => Err("gh took too long".to_owned()),
            }
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RerunRequest {
    key: String,
}

/// The run of a row of the fleet that may be started again: `(owner/name, id)`, or why not.
pub fn rerunnable(fleet: &Value, key: &str) -> Result<(String, u64), &'static str> {
    let row = fleet["repos"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["key"] == key))
        .ok_or("no_repo")?;
    let remote = &row["ci_remote"];
    let state = remote["state"].as_str().unwrap_or_default();
    if state == "rejected" {
        return Err("rejected");
    }
    if !RERUNNABLE.contains(&state) {
        return Err("not_red");
    }
    let repo = remote["repo"]
        .as_str()
        .filter(|r| plain_repo(r))
        .ok_or("no_run")?;
    let id = remote["run"]["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or("no_run")?;
    Ok((repo.to_owned(), id))
}

/// `POST /api/testing/ci-rerun` with `{"key": "<key of the fleet>"}`.
pub(crate) async fn ci_rerun(State(st): State<Arc<Shared>>, body: Bytes) -> Response {
    let Ok(request) = serde_json::from_slice::<RerunRequest>(&body) else {
        return error_body(StatusCode::BAD_REQUEST, "bad_body", "invalid request body");
    };
    if !is_repo_key(&request.key) {
        return error_body(StatusCode::BAD_REQUEST, "bad_key", "not a key of the fleet");
    }
    let rt = st.rt();
    let Some(rerun) = rt.rerun.clone() else {
        return error_body(
            StatusCode::SERVICE_UNAVAILABLE,
            "no_github",
            "this hub cannot start a run again (no gh)",
        );
    };
    let cached = match rt.store.current().await {
        Ok(cached) => cached,
        Err(error) => return crate::answer::engine_failure(error),
    };
    let fleet: Value = serde_json::from_slice(&cached.text).unwrap_or(Value::Null);
    let (repo, id) = match rerunnable(&fleet, &request.key) {
        Ok(found) => found,
        Err("rejected") => {
            return error_body(
                StatusCode::CONFLICT,
                "rejected",
                "the runner guard refused this run: release it on the PC (testing ci release)",
            );
        }
        Err("no_repo") => {
            return error_body(
                StatusCode::NOT_FOUND,
                "no_repo",
                "no such repository in the fleet",
            );
        }
        Err(code) => {
            return error_body(
                StatusCode::CONFLICT,
                code,
                "there is no red run to start again",
            );
        }
    };
    match rerun.rerun(&repo, id).await {
        Ok(()) => {
            rt.store.request_rescan();
            (StatusCode::ACCEPTED, axum::Json(json!({ "rerun": id }))).into_response()
        }
        Err(message) => error_body(StatusCode::BAD_GATEWAY, "rerun_failed", &message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fleet(state: &str, repo: &str, id: Value) -> Value {
        json!({ "repos": [{ "key": "lib.nvim-0123456789ab", "ci_remote": {
            "state": state, "repo": repo, "run": { "id": id } } }] })
    }

    #[test]
    fn only_a_red_run_of_the_fleet_is_started_again_and_never_a_rejected_one() {
        let key = "lib.nvim-0123456789ab";
        for state in RERUNNABLE {
            assert_eq!(
                rerunnable(&fleet(state, "WKDSteve/lib.nvim", json!(42)), key),
                Ok(("WKDSteve/lib.nvim".to_owned(), 42))
            );
        }
        assert_eq!(
            rerunnable(&fleet("rejected", "WKDSteve/lib.nvim", json!(42)), key),
            Err("rejected")
        );
        for state in ["success", "running", "queued", "unknown", ""] {
            assert_eq!(
                rerunnable(&fleet(state, "WKDSteve/lib.nvim", json!(42)), key),
                Err("not_red")
            );
        }
        for (repo, id) in [
            ("WKDSteve/lib.nvim; rm -rf", json!(42)),
            ("../x", json!(42)),
            ("WKDSteve/lib.nvim", json!(0)),
            ("WKDSteve/lib.nvim", json!("42")),
        ] {
            assert_eq!(
                rerunnable(&fleet("failure", repo, id), key),
                Err("no_run"),
                "{repo}"
            );
        }
        assert_eq!(
            rerunnable(
                &fleet("failure", "WKDSteve/lib.nvim", json!(42)),
                "other-0123456789ab"
            ),
            Err("no_repo")
        );
    }

    #[test]
    fn gh_is_asked_for_exactly_a_rerun_of_the_failed_jobs() {
        assert_eq!(
            gh_args("WKDSteve/lib.nvim", 42),
            ["run", "rerun", "42", "--failed", "-R", "WKDSteve/lib.nvim"]
        );
    }
}
