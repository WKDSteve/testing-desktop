//! The routes of a run through the engine (`launch`), below `/api/testing/run`:
//!
//! * `GET run`: where the run stands (a device sees less, `Launch::state_for_device`);
//! * `POST run/start` `{ "key": "<key of the fleet>", "file"?: "<part of a spec file name>" }`: the suite of a
//!   repository, or its spec files of one name; the PC only;
//! * `GET run/task-sheets?task=<area/slug>[&key=<key>]`: the sheets of the repository of a task that name the task;
//! * `POST run/task` `{ "task": "<area/slug>", "key"?: "<key of the fleet>" }`: run those sheets; for a paired device
//!   at the level `trigger`, confirmed on the device (`phone::DEVICE_ROUTES`).
//!
//! The repository of a task is a row of the fleet the hub holds: the one named by `key`, else the one whose name is
//! the area of the task (where the engine looks for the sheets of a task too, `$REPOS_DIR/<area>`). The hub holds the
//! fleet in its export form, so a device never reaches a repository the data classes do not release.

use std::path::PathBuf;
use std::sync::Arc;

use appkit_core::classes::Caller;
use appkit_hub::{error_body, json_response, no_query, parse_query};
use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::Extension;
use serde_json::{json, Value};

use crate::engine::is_repo_key;
use crate::launch::{is_file_text, is_task_id, task_area, task_sheets, StartError, What};
use crate::routes::Shared;
use crate::stepping::{object_of, root_of};

fn unavailable() -> Response {
    error_body(
        StatusCode::SERVICE_UNAVAILABLE,
        "run_unavailable",
        "this hub cannot start a run: no engine",
    )
}

/// `GET /api/testing/run`.
pub(crate) async fn state(
    State(st): State<Arc<Shared>>,
    RawQuery(query): RawQuery,
    caller: Option<Extension<Caller>>,
) -> Response {
    if let Some(response) = no_query(query.as_deref()) {
        return response;
    }
    let launch = &st.rt().launch;
    let body = match caller.as_deref() {
        Some(Caller::Device(_)) => launch.state_for_device(),
        _ => launch.state(),
    };
    json_response(StatusCode::OK, body.to_string(), None)
}

/// Only these fields.
fn fields(map: &serde_json::Map<String, Value>, allowed: &[&str]) -> Result<(), Box<Response>> {
    if map.keys().all(|k| allowed.contains(&k.as_str())) {
        Ok(())
    } else {
        Err(Box::new(error_body(
            StatusCode::BAD_REQUEST,
            "bad_body",
            "unknown field",
        )))
    }
}

/// An optional string field that passes `check`; `Err` when it is there and does not.
fn optional<'a>(
    map: &'a serde_json::Map<String, Value>,
    name: &str,
    check: fn(&str) -> bool,
) -> Result<Option<&'a str>, ()> {
    match map.get(name) {
        None => Ok(None),
        Some(Value::String(s)) if check(s) => Ok(Some(s)),
        Some(_) => Err(()),
    }
}

/// Start `what` in the repository `key`, and answer as a start does: `202` with the state, `409` while a run is
/// under way.
async fn launch(st: &Shared, key: &str, what: What, root: PathBuf) -> Response {
    match st.rt().launch.start(key, what, &root) {
        Ok(state) => json_response(StatusCode::ACCEPTED, state.to_string(), None),
        Err(StartError::Unavailable) => unavailable(),
        Err(StartError::Busy { what, key }) => error_body(
            StatusCode::CONFLICT,
            "busy",
            &format!("a run ({what}) of {key} is under way"),
        ),
        Err(StartError::Spawn(detail)) => {
            eprintln!("run: the engine could not be started: {detail}");
            error_body(
                StatusCode::BAD_GATEWAY,
                "engine_spawn",
                "the run could not be started",
            )
        }
    }
}

/// `POST /api/testing/run/abort` with `{}`: the run under way ends as `stopped`, with every process it started.
/// Answers `202` with the state (the run may still be on its way down; `run` says when it ended), `409 not_running`
/// when there is none. The PC only: no phone reaches it (`phone::DEVICE_ROUTES`).
pub(crate) async fn abort(State(st): State<Arc<Shared>>, body: Bytes) -> Response {
    let map = match object_of(&body) {
        Ok(map) => map,
        Err(r) => return *r,
    };
    if let Err(r) = fields(&map, &[]) {
        return *r;
    }
    let rt = st.rt();
    if !rt.launch.available() {
        return unavailable();
    }
    if !rt.launch.abort() {
        return error_body(StatusCode::CONFLICT, "not_running", "no run is under way");
    }
    json_response(StatusCode::ACCEPTED, rt.launch.state().to_string(), None)
}

/// `POST /api/testing/run/start`.
pub(crate) async fn start(State(st): State<Arc<Shared>>, body: Bytes) -> Response {
    let map = match object_of(&body) {
        Ok(map) => map,
        Err(r) => return *r,
    };
    if let Err(r) = fields(&map, &["key", "file"]) {
        return *r;
    }
    let Ok(Some(key)) = optional(&map, "key", is_repo_key) else {
        return error_body(
            StatusCode::BAD_REQUEST,
            "bad_key",
            "key must be a key of the fleet",
        );
    };
    let Ok(file) = optional(&map, "file", is_file_text) else {
        return error_body(
            StatusCode::BAD_REQUEST,
            "bad_file",
            "file must be part of a spec file name: letters, digits, _ - . /",
        );
    };
    if !st.rt().launch.available() {
        return unavailable();
    }
    let root = match root_of(&st, key).await {
        Ok(root) => root,
        Err(r) => return *r,
    };
    let what = file.map_or(What::Suite, |f| What::File(f.to_owned()));
    launch(&st, key, what, root).await
}

/// The key of the repository of a task: `key` when it is a row of the fleet, else the one row whose name is the area
/// of the task.
async fn task_repo(st: &Shared, task: &str, key: Option<&str>) -> Result<String, Box<Response>> {
    let rt = st.rt();
    if !rt.store.has_engine() {
        return Err(Box::new(unavailable()));
    }
    let cached = rt
        .store
        .current()
        .await
        .map_err(|e| Box::new(crate::answer::engine_failure(e)))?;
    let fleet: Value = serde_json::from_slice(&cached.text).unwrap_or(Value::Null);
    let rows = fleet["repos"].as_array().cloned().unwrap_or_default();
    let found: Vec<&str> = rows
        .iter()
        .filter(|r| match key {
            Some(key) => r["key"] == key,
            None => r["name"] == task_area(task),
        })
        .filter_map(|r| r["key"].as_str())
        .collect();
    match found.as_slice() {
        [one] => Ok((*one).to_owned()),
        [] => Err(Box::new(error_body(
            StatusCode::NOT_FOUND,
            "no_repo",
            "no repository of the fleet for this task (name it with key)",
        ))),
        _ => Err(Box::new(error_body(
            StatusCode::CONFLICT,
            "ambiguous",
            "more than one repository of the fleet has the name of this area (name it with key)",
        ))),
    }
}

/// The repository of a task, its root and the sheets there that name the task.
async fn sheets_of(
    st: &Shared,
    task: &str,
    key: Option<&str>,
) -> Result<(String, PathBuf, Vec<String>), Box<Response>> {
    let key = task_repo(st, task, key).await?;
    let root = root_of(st, &key).await?;
    let (dir, id) = (root.clone(), task.to_owned());
    match tokio::task::spawn_blocking(move || task_sheets(&dir, &id)).await {
        Ok(Ok(names)) => Ok((key, root, names)),
        _ => Err(Box::new(error_body(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unreadable",
            "the folder of the sheets cannot be read",
        ))),
    }
}

/// `GET /api/testing/run/task-sheets?task=<area/slug>[&key=<key>]`.
pub(crate) async fn task_sheets_route(
    State(st): State<Arc<Shared>>,
    RawQuery(query): RawQuery,
) -> Response {
    let params = match parse_query(query.as_deref(), &["task", "key"]) {
        Ok(params) => params,
        Err(why) => return error_body(StatusCode::BAD_REQUEST, "bad_query", why),
    };
    let Some(task) = params.get("task").filter(|t| is_task_id(t)) else {
        return error_body(
            StatusCode::BAD_REQUEST,
            "bad_task",
            "task must be the id of a task, area/slug",
        );
    };
    let key = params.get("key").map(String::as_str);
    if key.is_some_and(|k| !is_repo_key(k)) {
        return error_body(StatusCode::BAD_REQUEST, "bad_key", "not a key of the fleet");
    }
    match sheets_of(&st, task, key).await {
        Ok((key, _, sheets)) => json_response(
            StatusCode::OK,
            json!({ "schema": 1, "kind": "hub.task-sheets", "task": task, "key": key, "sheets": sheets })
                .to_string(),
            None,
        ),
        Err(r) => *r,
    }
}

/// `POST /api/testing/run/task`.
pub(crate) async fn task(State(st): State<Arc<Shared>>, body: Bytes) -> Response {
    let map = match object_of(&body) {
        Ok(map) => map,
        Err(r) => return *r,
    };
    if let Err(r) = fields(&map, &["task", "key"]) {
        return *r;
    }
    let Ok(Some(task)) = optional(&map, "task", is_task_id) else {
        return error_body(
            StatusCode::BAD_REQUEST,
            "bad_task",
            "task must be the id of a task, area/slug",
        );
    };
    let Ok(key) = optional(&map, "key", is_repo_key) else {
        return error_body(StatusCode::BAD_REQUEST, "bad_key", "not a key of the fleet");
    };
    if !st.rt().launch.available() {
        return unavailable();
    }
    let (key, root, sheets) = match sheets_of(&st, task, key).await {
        Ok(found) => found,
        Err(r) => return *r,
    };
    // Only a task the hub knows sheets of is run: the run of a task is the run of its sheets, nothing else.
    if sheets.is_empty() {
        return error_body(
            StatusCode::NOT_FOUND,
            "no_sheet",
            "no sheet of the repository names this task",
        );
    }
    launch(&st, &key, What::Task(task.to_owned()), root).await
}
