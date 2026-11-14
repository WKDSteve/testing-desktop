//! The routes of the release assistant for the runner guard (`release`), below `/api/testing/release`:
//!
//! * `GET release/repos`: the repositories a commit can be released of: those on the engine's list of reviewed
//!   commits, and those of the fleet that GitHub knows (`ci_remote.repo`), each with the state of its CI;
//! * `GET release/commits?repo=<owner/name>`: the last commits of its default branch, each with `released`;
//! * `GET release/show?repo=<owner/name>&sha=<40 hex>`: the review aid of one commit (what changed since the last
//!   reviewed one, the files that deserve a close look). Handing it out is what allows the release of that commit;
//! * `POST release` `{ "repo": "<owner/name>", "sha": "<40 hex>" }`: start the release of that commit and the re-run
//!   of its failed runs. `202`: it runs in the background, to its end whatever becomes of the page;
//! * `GET release/outcome`: where the release that was started last stands, and what it did.
//!
//! Every step is the engine's (`testing ci release ... --json`); an answer is `{ "schema": 1, "kind":
//! "hub.release.<step>", "exit", "problem", ... }` with the part of the engine's document for the step. `exit` is the
//! engine's: `0` done, `2` it refused the arguments, `3` GitHub or the list failed (`problem` says which). The PC
//! only: no phone reaches a route of this file.

use std::sync::Arc;

use appkit_hub::{error_body, json_response, no_query, parse_query};
use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{json, Value};

use crate::release::{is_full_sha, is_repo, BeginError, Outcome, Step, StepError};
use crate::rerun::RERUNNABLE;
use crate::routes::Shared;
use crate::stepping::object_of;

fn unavailable() -> Response {
    error_body(
        StatusCode::SERVICE_UNAVAILABLE,
        "release_unavailable",
        "this hub cannot release a commit: no engine",
    )
}

/// The answer to a step that gave no document.
fn failed(error: &StepError) -> Response {
    match error {
        StepError::Spawn(detail) => {
            eprintln!("release: the engine could not be started: {detail}");
        }
        StepError::Protocol(why) => eprintln!("release: the engine gave no document: {why}"),
        _ => {}
    }
    let (code, message) = error.words();
    let status = match error {
        StepError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        StepError::Busy => StatusCode::CONFLICT,
        StepError::Timeout => StatusCode::GATEWAY_TIMEOUT,
        StepError::Spawn(_) | StepError::Protocol(_) => StatusCode::BAD_GATEWAY,
    };
    error_body(status, code, message)
}

/// The frame of an answer: the kind, and the exit code and problem of the engine's document.
fn answer(step: &str, doc: &Value) -> Value {
    json!({
        "schema": 1,
        "kind": format!("hub.release.{step}"),
        "exit": doc["exit"],
        "problem": doc["problem"],
    })
}

fn ok(body: &Value) -> Response {
    json_response(StatusCode::OK, body.to_string(), None)
}

/// The repositories of the fleet GitHub knows, with the state of their CI: `(owner/name, state)`.
async fn fleet_repos(st: &Shared) -> Vec<(String, String)> {
    let rt = st.rt();
    if !rt.store.has_engine() {
        return Vec::new();
    }
    let Ok(cached) = rt.store.current().await else {
        return Vec::new();
    };
    let fleet: Value = serde_json::from_slice(&cached.text).unwrap_or(Value::Null);
    fleet["repos"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let remote = &row["ci_remote"];
                    let name = remote["repo"].as_str().filter(|r| is_repo(r))?;
                    Some((
                        name.to_owned(),
                        remote["state"].as_str().unwrap_or_default().to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The rows of `release/repos`: the engine's list first as it names them, then the repositories of the fleet it does
/// not hold; a repository of both is one row. `red`: its CI has a run that failed, the place a refused job shows.
pub(crate) fn merged_repos(listed: &Value, fleet: &[(String, String)]) -> Vec<Value> {
    let mut rows: Vec<Value> = Vec::new();
    let state_of = |name: &str| {
        fleet
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, state)| state.as_str())
    };
    for repo in listed.as_array().into_iter().flatten() {
        let Some(name) = repo["name"].as_str().filter(|n| is_repo(n)) else {
            continue;
        };
        if rows.iter().any(|r| {
            r["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        }) {
            continue;
        }
        let state = state_of(name);
        rows.push(json!({
            "name": name,
            "listed": repo["mode"].is_string(),
            "mode": repo["mode"],
            "reviewed": repo["reviewed"].as_u64().unwrap_or(0),
            "ci": state,
            "red": state.is_some_and(|s| RERUNNABLE.contains(&s)),
        }));
    }
    for (name, state) in fleet {
        if rows.iter().any(|r| {
            r["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        }) {
            continue;
        }
        rows.push(json!({
            "name": name,
            "listed": false,
            "mode": null,
            "reviewed": 0,
            "ci": state,
            "red": RERUNNABLE.contains(&state.as_str()),
        }));
    }
    // Red first: that is where a refused job shows. Then as they came (the list, then the fleet by name).
    rows.sort_by_key(|r| r["red"] != true);
    rows
}

/// `GET /api/testing/release/repos`.
pub(crate) async fn repos(State(st): State<Arc<Shared>>, RawQuery(query): RawQuery) -> Response {
    if let Some(response) = no_query(query.as_deref()) {
        return response;
    }
    let release = Arc::clone(&st.rt().release);
    if !release.available() {
        return unavailable();
    }
    let doc = match release.run(&Step::Repos).await {
        Ok(doc) => doc,
        Err(error) => return failed(&error),
    };
    let rows = merged_repos(&doc["repos"], &fleet_repos(&st).await);
    release.set_known(
        rows.iter()
            .filter_map(|r| r["name"].as_str().map(str::to_owned)),
    );
    let mut body = answer("repos", &doc);
    body["repos"] = Value::Array(rows);
    ok(&body)
}

/// The repository of a request, when it is one the assistant offered.
fn known_repo<'a>(st: &Shared, repo: Option<&'a str>) -> Result<&'a str, Box<Response>> {
    let Some(repo) = repo.filter(|r| is_repo(r)) else {
        return Err(Box::new(error_body(
            StatusCode::BAD_REQUEST,
            "bad_repo",
            "repo must be a repository, owner/name",
        )));
    };
    if !st.rt().release.is_known(repo) {
        return Err(Box::new(error_body(
            StatusCode::NOT_FOUND,
            "unknown_repo",
            "not a repository the assistant offered (read release/repos first)",
        )));
    }
    Ok(repo)
}

fn bad_sha() -> Response {
    error_body(
        StatusCode::BAD_REQUEST,
        "bad_sha",
        "sha must be the full commit id: 40 hex digits",
    )
}

/// `GET /api/testing/release/commits?repo=<owner/name>`.
pub(crate) async fn commits(State(st): State<Arc<Shared>>, RawQuery(query): RawQuery) -> Response {
    let params = match parse_query(query.as_deref(), &["repo"]) {
        Ok(params) => params,
        Err(why) => return error_body(StatusCode::BAD_REQUEST, "bad_query", why),
    };
    let release = Arc::clone(&st.rt().release);
    if !release.available() {
        return unavailable();
    }
    let repo = match known_repo(&st, params.get("repo").map(String::as_str)) {
        Ok(repo) => repo,
        Err(r) => return *r,
    };
    let doc = match release
        .run(&Step::Commits {
            repo: repo.to_owned(),
        })
        .await
    {
        Ok(doc) => doc,
        Err(error) => return failed(&error),
    };
    let mut body = answer("commits", &doc);
    body["repo"] = json!(repo);
    // Only what is a full commit id can be chosen: the release takes no other.
    body["commits"] = doc["commits"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter(|c| c["sha"].as_str().is_some_and(is_full_sha))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into();
    ok(&body)
}

/// `GET /api/testing/release/show?repo=<owner/name>&sha=<40 hex>`.
pub(crate) async fn show(State(st): State<Arc<Shared>>, RawQuery(query): RawQuery) -> Response {
    let params = match parse_query(query.as_deref(), &["repo", "sha"]) {
        Ok(params) => params,
        Err(why) => return error_body(StatusCode::BAD_REQUEST, "bad_query", why),
    };
    let release = Arc::clone(&st.rt().release);
    if !release.available() {
        return unavailable();
    }
    let repo = match known_repo(&st, params.get("repo").map(String::as_str)) {
        Ok(repo) => repo,
        Err(r) => return *r,
    };
    let Some(sha) = params.get("sha").filter(|s| is_full_sha(s)) else {
        return bad_sha();
    };
    let step = Step::Show {
        repo: repo.to_owned(),
        sha: sha.clone(),
    };
    let doc = match release.run(&step).await {
        Ok(doc) => doc,
        Err(error) => return failed(&error),
    };
    let mut body = answer("show", &doc);
    body["repo"] = json!(repo);
    body["sha"] = json!(sha);
    // The aid counts only when the engine found exactly this commit in this repository and says what it compared.
    let shown = doc["exit"] == 0 && doc["sha"] == sha.as_str() && doc["show"].is_object();
    if shown {
        body["show"] = doc["show"].clone();
        release.remember_shown(repo, sha);
    } else {
        body["show"] = Value::Null;
    }
    ok(&body)
}

/// `POST /api/testing/release` `{ "repo": "<owner/name>", "sha": "<40 hex>" }`.
pub(crate) async fn release(State(st): State<Arc<Shared>>, body: Bytes) -> Response {
    let map = match object_of(&body) {
        Ok(map) => map,
        Err(r) => return *r,
    };
    if map.keys().any(|k| k != "repo" && k != "sha") {
        return error_body(StatusCode::BAD_REQUEST, "bad_body", "unknown field");
    }
    let release = Arc::clone(&st.rt().release);
    if !release.available() {
        return unavailable();
    }
    let repo = match known_repo(&st, map.get("repo").and_then(Value::as_str)) {
        Ok(repo) => repo,
        Err(r) => return *r,
    };
    let Some(sha) = map
        .get("sha")
        .and_then(Value::as_str)
        .filter(|s| is_full_sha(s))
    else {
        return bad_sha();
    };
    // Only the commit a person was shown (its review aid is the one handed out last, and it is spent here), and not
    // tied to this request: a release runs to its end.
    let rt = st.rt();
    let store = Arc::clone(&rt.store);
    // The runs of the commit start again: the fleet is read again when the release ended.
    match release.begin_release(repo, sha, move || store.request_rescan()) {
        Ok(()) => json_response(
            StatusCode::ACCEPTED,
            json!({ "schema": 1, "kind": "hub.release.started", "repo": repo, "sha": sha })
                .to_string(),
            None,
        ),
        Err(BeginError::Unavailable) => unavailable(),
        Err(BeginError::Busy) => failed(&StepError::Busy),
        Err(BeginError::NotShown) => error_body(
            StatusCode::CONFLICT,
            "not_shown",
            "the review aid of this commit was not read just now (read release/show for it first)",
        ),
    }
}

/// `GET /api/testing/release/outcome`: where the release that was started last stands. `state` is `none` (no release
/// was started since the hub runs), `pending`, `done` (with the engine's `exit`, `problem` and `release`) or `failed`
/// (the step gave no document: `error` says why).
pub(crate) async fn outcome(State(st): State<Arc<Shared>>, RawQuery(query): RawQuery) -> Response {
    if let Some(response) = no_query(query.as_deref()) {
        return response;
    }
    let mut body = json!({ "schema": 1, "kind": "hub.release.outcome", "state": "none" });
    if let Some(last) = st.rt().release.last() {
        body["repo"] = json!(last.repo);
        body["sha"] = json!(last.sha);
        match last.outcome {
            Outcome::Pending => body["state"] = json!("pending"),
            Outcome::Done(doc) => {
                body["state"] = json!("done");
                body["exit"] = doc["exit"].clone();
                body["problem"] = doc["problem"].clone();
                body["release"] = if doc["release"].is_object() {
                    doc["release"].clone()
                } else {
                    Value::Null
                };
            }
            Outcome::Failed { code, message } => {
                body["state"] = json!("failed");
                body["error"] = json!({ "code": code, "message": message });
            }
        }
    }
    ok(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repositories_are_the_list_and_the_fleet_each_once_and_red_first() {
        let listed = json!([
            { "name": "WKDSteve/testing.nvim", "here": false, "mode": "auto", "reviewed": 3 },
            { "name": "o/elsewhere", "here": true, "mode": null, "reviewed": 0 },
            { "name": "not a repo", "mode": "auto", "reviewed": 1 },
        ]);
        let fleet = vec![
            ("wkdsteve/Testing.nvim".to_owned(), "success".to_owned()),
            ("WKDSteve/lib.nvim".to_owned(), "failure".to_owned()),
            ("WKDSteve/ui.nvim".to_owned(), "rejected".to_owned()),
        ];
        let rows = merged_repos(&listed, &fleet);
        let names: Vec<&str> = rows.iter().filter_map(|r| r["name"].as_str()).collect();
        assert_eq!(
            names,
            [
                "WKDSteve/lib.nvim",
                "WKDSteve/testing.nvim",
                "o/elsewhere",
                "WKDSteve/ui.nvim"
            ]
        );
        assert_eq!(
            rows[0],
            json!({ "name": "WKDSteve/lib.nvim", "listed": false, "mode": null, "reviewed": 0,
                    "ci": "failure", "red": true })
        );
        assert_eq!(
            rows[1],
            json!({ "name": "WKDSteve/testing.nvim", "listed": true, "mode": "auto", "reviewed": 3,
                    "ci": "success", "red": false })
        );
        assert_eq!(rows[2]["listed"], false);
        assert_eq!(rows[2]["ci"], Value::Null);
        // A workflow GitHub refused is not a job the guard refused: no release starts it.
        assert_eq!(rows[3]["red"], false);
        assert!(merged_repos(&Value::Null, &[]).is_empty());
    }
}
