//! The routes of a stepped run of a test sheet (`sheetstep`), below `/api/testing/sheet-step`: the sheets of a
//! repository, the start of a run, a command to it, where it stands, and a picture of a step. The first routes of
//! this hub that start something: a POST names a repository by its key of the fleet and a sheet by its name, and a
//! command is one of a closed list; nothing else of a request reaches the child.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use appkit_hub::{error_body, json_response, no_query, parse_query};
use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::header::{self, HeaderValue};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

use crate::answer::{engine_failure, engine_unavailable};
use crate::engine::{is_repo_key, Calls, EngineError};
use crate::routes::Shared;
use crate::sheetstep::{
    is_sheet_name, is_size, list_sheets, Command, CommandError, StartError, MAX_IMAGE_BYTES,
};

/// The longest body of a POST here: a key, a name and a word; a line of an answer.
const MAX_BODY: usize = 4096;

/// The root of the repository with this key, as the engine names it in the local form of `repo`. A key the
/// engine finds no repository for is `404`.
pub(crate) async fn root_of(st: &Shared, key: &str) -> Result<PathBuf, Box<Response>> {
    let rt = st.rt();
    let Some(engine) = &rt.engine else {
        return Err(Box::new(engine_unavailable(&rt)));
    };
    let reply = match engine
        .call(Calls::Repo, Some(&rt.ask.repo_local(key)))
        .await
    {
        Ok(reply) => reply,
        Err(EngineError::Failed { code: Some(2), .. }) => {
            return Err(Box::new(error_body(
                StatusCode::NOT_FOUND,
                "not_found",
                "no repository with this key",
            )));
        }
        Err(error) => return Err(Box::new(engine_failure(error))),
    };
    let doc: Value = serde_json::from_str(&reply.text).unwrap_or(Value::Null);
    let root = doc["repo"]["root"]
        .as_str()
        .filter(|_| doc["form"] == "local")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_dir());
    root.ok_or_else(|| {
        Box::new(error_body(
            StatusCode::BAD_GATEWAY,
            "engine_protocol",
            "the engine named no folder for the repository",
        ))
    })
}

/// The key of `?key=<key>`, as `repo` takes it.
fn key_of(query: Option<&str>) -> Result<String, Box<Response>> {
    crate::routes::repo_key(query)
        .map(str::to_owned)
        .map_err(|why| Box::new(error_body(StatusCode::BAD_REQUEST, "bad_query", why)))
}

/// The body of a POST as one JSON object; a body that is not is refused before anything is read from it.
pub(crate) fn object_of(body: &Bytes) -> Result<serde_json::Map<String, Value>, Box<Response>> {
    if body.len() > MAX_BODY {
        return Err(Box::new(error_body(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            "the body is larger than this route takes",
        )));
    }
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(Box::new(error_body(
            StatusCode::BAD_REQUEST,
            "bad_body",
            "the body must be a JSON object",
        ))),
    }
}

/// `GET /api/testing/sheet-step/sheets?key=<key>`: the names of the sheets of a repository (the files below its
/// `TESTS/sheets`), nothing of their content.
pub(crate) async fn sheets(State(st): State<Arc<Shared>>, RawQuery(query): RawQuery) -> Response {
    let key = match key_of(query.as_deref()) {
        Ok(key) => key,
        Err(r) => return *r,
    };
    if !st.rt().sheet_step.available() {
        return unavailable();
    }
    let root = match root_of(&st, &key).await {
        Ok(root) => root,
        Err(r) => return *r,
    };
    let listed = tokio::task::spawn_blocking(move || list_sheets(&root)).await;
    match listed {
        Ok(Ok(names)) => json_response(
            StatusCode::OK,
            json!({ "schema": 1, "kind": "hub.sheets", "key": key, "sheets": names }).to_string(),
            None,
        ),
        Ok(Err(e)) => {
            eprintln!("sheet step: the sheets of a repository cannot be listed: {e}");
            error_body(
                StatusCode::INTERNAL_SERVER_ERROR,
                "unreadable",
                "the folder of the sheets cannot be read",
            )
        }
        Err(_) => error_body(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unreadable",
            "the folder of the sheets cannot be read",
        ),
    }
}

fn unavailable() -> Response {
    error_body(
        StatusCode::SERVICE_UNAVAILABLE,
        "step_unavailable",
        "this hub cannot run a sheet: no engine",
    )
}

/// `GET /api/testing/sheet-step`: where the run stands, with the events a page needs to show it.
pub(crate) async fn state(State(st): State<Arc<Shared>>, RawQuery(query): RawQuery) -> Response {
    if let Some(response) = no_query(query.as_deref()) {
        return response;
    }
    json_response(StatusCode::OK, st.rt().sheet_step.state().to_string(), None)
}

/// `POST /api/testing/sheet-step/start` `{ "key": "<key of the fleet>", "sheet": "<name>" }`: run that sheet of that
/// repository step by step. One run per hub: a second is `409` and names the sheet that runs. The engine's refusal
/// (the workstation, an untrusted root, no such sheet) is `422` with its reason.
pub(crate) async fn start(State(st): State<Arc<Shared>>, body: Bytes) -> Response {
    let map = match object_of(&body) {
        Ok(map) => map,
        Err(r) => return *r,
    };
    for field in map.keys() {
        if field != "key" && field != "sheet" {
            return error_body(StatusCode::BAD_REQUEST, "bad_body", "unknown field");
        }
    }
    let Some(key) = map
        .get("key")
        .and_then(Value::as_str)
        .filter(|k| is_repo_key(k))
    else {
        return error_body(
            StatusCode::BAD_REQUEST,
            "bad_key",
            "key must be a key of the fleet",
        );
    };
    let Some(sheet) = map
        .get("sheet")
        .and_then(Value::as_str)
        .filter(|s| is_sheet_name(s))
    else {
        return error_body(
            StatusCode::BAD_REQUEST,
            "bad_sheet",
            "sheet must be the name of a sheet below TESTS/sheets",
        );
    };
    let step = Arc::clone(&st.rt().sheet_step);
    if !step.available() {
        return unavailable();
    }
    let root = match root_of(&st, key).await {
        Ok(root) => root,
        Err(r) => return *r,
    };
    match step.start(key, sheet, &root).await {
        Ok(state) => json_response(StatusCode::OK, state.to_string(), None),
        Err(StartError::Unavailable) => unavailable(),
        Err(StartError::Busy { sheet }) => error_body(
            StatusCode::CONFLICT,
            "busy",
            &format!("a run of {sheet} is under way"),
        ),
        Err(StartError::Spawn(detail)) => {
            eprintln!("sheet step: the child could not be started: {detail}");
            error_body(
                StatusCode::BAD_GATEWAY,
                "engine_spawn",
                "the run could not be started",
            )
        }
        Err(StartError::Refused { code, detail }) => {
            let body = json!({ "error": {
                "code": "refused",
                "message": if detail.is_empty() { "the engine refused the run".to_owned() } else { detail },
                "exit": code,
            }});
            (StatusCode::UNPROCESSABLE_ENTITY, axum::Json(body)).into_response()
        }
    }
}

/// `POST /api/testing/sheet-step/command` `{ "command": "next" | "continue" | "until" | "break" | "clear" |
/// "answer" | "abort", "n"?: <step>, "yes"?: <bool>, "text"?: "<line>" }`: one command of the list to the run.
pub(crate) async fn command(State(st): State<Arc<Shared>>, body: Bytes) -> Response {
    let map = match object_of(&body) {
        Ok(map) => map,
        Err(r) => return *r,
    };
    let command = match Command::parse(&Value::Object(map)) {
        Ok(command) => command,
        Err(why) => return error_body(StatusCode::BAD_REQUEST, "bad_command", &why),
    };
    let step = Arc::clone(&st.rt().sheet_step);
    match step.command(&command) {
        Ok(()) => json_response(StatusCode::OK, step.state().to_string(), None),
        Err(CommandError::NotRunning) => {
            error_body(StatusCode::CONFLICT, "not_running", "no run is under way")
        }
    }
}

/// The media type of a picture by the name of its file; only these two are served.
fn image_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        _ => None,
    }
}

/// `GET /api/testing/sheet-step/image?size=<cols>x<rows>&n=<step>&kind=image|shot`: the picture the child named
/// for that step of the run (`image`: the target after the step; `shot`: the file of a `shot` step). The request
/// names a step, never a file; a step the child named no picture for is `404`.
pub(crate) async fn image(State(st): State<Arc<Shared>>, RawQuery(query): RawQuery) -> Response {
    let params = match parse_query(query.as_deref(), &["size", "n", "kind"]) {
        Ok(params) => params,
        Err(why) => return error_body(StatusCode::BAD_REQUEST, "bad_query", why),
    };
    let size = params.get("size").filter(|s| is_size(s));
    let n = params.get("n").and_then(|n| n.parse::<u64>().ok());
    let kind = params
        .get("kind")
        .filter(|k| matches!(k.as_str(), "image" | "shot"));
    let (Some(size), Some(n), Some(kind)) = (size, n, kind) else {
        return error_body(
            StatusCode::BAD_REQUEST,
            "bad_query",
            "size, n and kind (image or shot) name a step",
        );
    };
    let Some(path) = st.rt().sheet_step.picture(size, n, kind) else {
        return error_body(
            StatusCode::NOT_FOUND,
            "not_found",
            "the run has no picture of this step",
        );
    };
    let Some(media) = image_type(&path) else {
        return error_body(
            StatusCode::NOT_FOUND,
            "not_found",
            "the picture of this step is of a kind this hub does not serve",
        );
    };
    let bytes = async {
        let meta = tokio::fs::metadata(&path).await?;
        if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES {
            return Err(std::io::Error::other("not a file to serve"));
        }
        tokio::fs::read(&path).await
    }
    .await;
    match bytes {
        Ok(bytes) => {
            let mut response = (StatusCode::OK, bytes).into_response();
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(media));
            response
        }
        Err(_) => error_body(
            StatusCode::NOT_FOUND,
            "not_found",
            "the picture of this step is not there",
        ),
    }
}
