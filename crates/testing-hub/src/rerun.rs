//! `POST /api/testing/ci-rerun`: start the failed jobs of a red CI run again, the one thing a phone may start
//! (`phone::DEVICE_ROUTES`, level `trigger`, confirmed on the phone per request).
//!
//! The request names a repository of the fleet by its key, nothing else. The hub looks the run up in the fleet it
//! holds: the latest run of the CI workflow for the head of origin (`ci_remote`), and only when it failed, was
//! cancelled or found no runner. A run GitHub refused (`rejected`: it did not take the workflow file, the run failed
//! without a job, `docs/CI.md` of testing.nvim) is not started again: it has no failed job, and the same file would be
//! refused again. A job the runner guard refused is another thing: it shows as `failure` and is released on the PC
//! (the release assistant, `testing ci release`), where the person looks at the commit. The hub then asks GitHub's REST
//! API to start the failed jobs again (`POST /repos/<owner>/<repo>/actions/runs/<id>/rerun-failed-jobs`, what
//! `gh run rerun --failed` asks), with the token it reads GitHub with, under a time limit: no process is started.
//! Nothing of the request reaches GitHub but the key, and the key only chooses a row of the fleet.

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
/// What the hub says to a run GitHub refused (`rejected`).
pub const REJECTED: &str =
    "GitHub refused the workflow file of this run (it failed without a job): there is no job to start again, fix the workflow file on the PC";
/// Where GitHub's REST API is.
pub const GITHUB_API: &str = "https://api.github.com";
/// How long GitHub may take to answer.
const API_TIMEOUT: Duration = Duration::from_secs(30);

/// Starts the failed jobs of a run again. The program's is [`GhRerun`]; a test brings its own.
pub trait Rerunner: Send + Sync {
    /// Rerun the failed jobs of run `id` of `repo` (`owner/name`).
    fn rerun(&self, repo: &str, id: u64) -> BoxFuture<'_, Result<(), String>>;
}

/// The path of GitHub's REST API that starts the failed jobs of run `id` of `repo` again: a fixed path, the repository
/// and the id, nothing else.
#[must_use]
pub fn rerun_path(repo: &str, id: u64) -> String {
    format!("/repos/{repo}/actions/runs/{id}/rerun-failed-jobs")
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

/// GitHub's REST API, with the token the hub reads GitHub with (from `github_token_file`, or the one `gh auth token`
/// gave at the start).
pub struct ApiRerun {
    api: String,
    token: GithubToken,
    client: reqwest::Client,
}

impl ApiRerun {
    /// `api` is [`GITHUB_API`]; a test names its stand-in.
    ///
    /// # Errors
    ///
    /// The HTTP client could not be built (no TLS).
    pub fn new(api: &str, token: GithubToken) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(API_TIMEOUT)
            // An answer that sends the request elsewhere is not followed: the token goes to GitHub only.
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("testing-hub/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| format!("no HTTP client for GitHub: {e}"))?;
        Ok(Self {
            api: api.trim_end_matches('/').to_owned(),
            token,
            client,
        })
    }
}

impl Rerunner for ApiRerun {
    fn rerun(&self, repo: &str, id: u64) -> BoxFuture<'_, Result<(), String>> {
        // `rerunnable` checked both; the URL is built only of what passes again.
        let url =
            (plain_repo(repo) && id > 0).then(|| format!("{}{}", self.api, rerun_path(repo, id)));
        Box::pin(async move {
            let Some(url) = url else {
                return Err("not a run of GitHub".to_owned());
            };
            let response = self
                .client
                .post(url)
                .bearer_auth(self.token.expose())
                .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .send()
                .await
                .map_err(|e| {
                    if e.is_timeout() {
                        "GitHub took too long".to_owned()
                    } else {
                        "GitHub could not be reached".to_owned()
                    }
                })?;
            // Nothing of the answer is passed on but its status.
            match response.status().as_u16() {
                201 => Ok(()),
                401 => Err("GitHub did not take the token (HTTP 401)".to_owned()),
                code @ (403 | 404) => Err(format!(
                    "GitHub refused the rerun (HTTP {code}): the token needs Actions write for this repository"
                )),
                code => Err(format!("GitHub refused the rerun (HTTP {code})")),
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
            "this hub cannot start a run again (no GitHub token)",
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
            return error_body(StatusCode::CONFLICT, "rejected", REJECTED);
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
    fn a_rejected_run_is_named_as_what_it_is_a_workflow_file_github_refused() {
        // `rejected` is GitHub's refusal of the workflow file (testing.nvim, docs/CI.md), never the runner guard's
        // refusal of a job (that is a `failure`, released with `testing ci release`).
        let said = REJECTED.to_lowercase();
        assert!(said.contains("workflow file"), "{REJECTED}");
        for wrong in ["guard", "release"] {
            assert!(!said.contains(wrong), "{REJECTED}");
        }
    }

    #[test]
    fn github_is_asked_for_exactly_a_rerun_of_the_failed_jobs() {
        assert_eq!(
            rerun_path("WKDSteve/lib.nvim", 42),
            "/repos/WKDSteve/lib.nvim/actions/runs/42/rerun-failed-jobs"
        );
    }

    const TOKEN: &str = "github_pat_0123456789abcdefABCDEF";

    /// A stand-in for GitHub's API: answers one request with `status` and hands back what it was asked.
    async fn stand_in(status: &'static str) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = format!("http://{}", listener.local_addr().unwrap());
        let asked = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0, "the request ended early");
                request.extend_from_slice(&buf[..n]);
            }
            let answer =
                format!("HTTP/1.1 {status}\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}");
            socket.write_all(answer.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
            String::from_utf8(request).unwrap()
        });
        (api, asked)
    }

    #[tokio::test]
    async fn a_rerun_is_one_post_to_the_api_with_the_token_of_the_hub_and_no_process() {
        let (api, asked) = stand_in("201 Created").await;
        let rerun = ApiRerun::new(&api, GithubToken::parse(TOKEN).unwrap()).unwrap();
        rerun.rerun("WKDSteve/lib.nvim", 42).await.unwrap();
        let request = asked.await.unwrap();
        assert_eq!(
            request.lines().next(),
            Some("POST /repos/WKDSteve/lib.nvim/actions/runs/42/rerun-failed-jobs HTTP/1.1")
        );
        let request = request.to_ascii_lowercase();
        for header in [
            format!("authorization: bearer {}", TOKEN.to_ascii_lowercase()),
            "accept: application/vnd.github+json".to_owned(),
            "x-github-api-version: 2022-11-28".to_owned(),
            "user-agent: testing-hub/".to_owned(),
        ] {
            assert!(request.contains(&header), "{header}");
        }
    }

    #[tokio::test]
    async fn a_refusal_of_github_says_its_status_and_a_run_that_is_none_is_not_asked() {
        let (api, asked) = stand_in("403 Forbidden").await;
        let rerun = ApiRerun::new(&api, GithubToken::parse(TOKEN).unwrap()).unwrap();
        let said = rerun.rerun("WKDSteve/lib.nvim", 42).await.unwrap_err();
        assert!(
            said.contains("HTTP 403") && said.contains("Actions write"),
            "{said}"
        );
        asked.await.unwrap();
        // A repository or an id that is not one of GitHub never leaves the hub (port 9 has nobody to answer).
        let nowhere =
            ApiRerun::new("http://127.0.0.1:9", GithubToken::parse(TOKEN).unwrap()).unwrap();
        for (repo, id) in [("../x", 42), ("a/b/c", 42), ("WKDSteve/lib.nvim", 0)] {
            assert_eq!(
                nowhere.rerun(repo, id).await.unwrap_err(),
                "not a run of GitHub",
                "{repo} {id}"
            );
        }
    }
}
