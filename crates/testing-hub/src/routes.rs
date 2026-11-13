//! The routes of the app, under `/api/testing/`. Each is under `/api/`, so the guard asked for a
//! session before a handler runs. The documents of the engine are passed on as the engine wrote
//! them; their `digest` is the ETag. The fleet is the snapshot the hub holds; a repository and its
//! findings are asked for when they are wanted.
//!
//! [`ROUTES`] is the one list of them: the router is built from it and the security matrix reads
//! its names, so a route cannot be served without the matrix trying it without a session.
//!
//! The routes of a stepped run of a sheet (`sheet-step`, in `stepping`) start the engine's `testing sheets step` in
//! a repository of the fleet and pass it a command of a closed list; those of a run through the engine (`run/*`, in
//! `launching`) start its command line for a suite, a file or the sheets of a task; `ci-rerun` starts the failed
//! jobs of a red CI run again.

use std::sync::Arc;

use appkit_hub::{error_body, etag_matches, json_response, no_query, not_modified, single, Core};
use axum::extract::{RawQuery, State};
use axum::http::header::{self, HeaderMap};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post, MethodRouter};
use axum::Router;
use serde_json::{json, Value};

use crate::answer::{engine_failure, engine_unavailable};
use crate::engine::{is_repo_key, Ask, Calls, EngineError, Method, Reply};
use crate::runtime::TestingRuntime;
use crate::{launching, live, stepping, IDENTITY};

/// The core of the hub with the runtime of this app.
pub type Shared = Core<TestingRuntime>;

/// How one route is answered: its method and handler.
pub type MakeRoute = fn() -> MethodRouter<Arc<Shared>>;

/// Every route of the app, by its name below `/api/testing/`.
pub const ROUTES: &[(&str, MakeRoute)] = &[
    ("fleet", || get(fleet)),
    ("repo", || get(repo)),
    ("finding", || get(finding)),
    ("events", || get(live::events)),
    ("about", || get(about)),
    ("sheet-step", || get(stepping::state)),
    ("sheet-step/sheets", || get(stepping::sheets)),
    ("sheet-step/start", || post(stepping::start)),
    ("sheet-step/command", || post(stepping::command)),
    ("sheet-step/image", || get(stepping::image)),
    ("ci-rerun", || post(crate::rerun::ci_rerun)),
    ("run", || get(launching::state)),
    ("run/start", || post(launching::start)),
    ("run/task-sheets", || get(launching::task_sheets_route)),
    ("run/task", || post(launching::task)),
];

/// The router of the app, built from [`ROUTES`] and from nothing else. Invariant: this stays a
/// pure fold over the table, with no extra `route` or `merge`, because [`crate::DATA_ROUTES`] and
/// the security matrix only know the routes of the table; a route added beside it would be served
/// without being tried there.
pub(crate) fn app_routes(_core: &Arc<Shared>) -> Router<Arc<Shared>> {
    ROUTES.iter().fold(Router::new(), |router, (name, make)| {
        router.route(&IDENTITY.route(name), make())
    })
}

/// The `digest` of a document of the engine, when it has one of the contract's form
/// (`sha256:<hex>`): the ETag of its answer.
fn digest_of(text: &str) -> Option<String> {
    let doc: Value = serde_json::from_str(text).ok()?;
    let digest = doc.get("digest")?.as_str()?;
    let hex = digest.strip_prefix("sha256:")?;
    (!hex.is_empty() && hex.len() <= 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| digest.to_owned())
}

/// A document of the engine as the answer: with its digest as the ETag, and `304` when the client
/// already holds that one.
fn document(headers: &HeaderMap, reply: Reply) -> Response {
    let etag = digest_of(&reply.text);
    if let (Some(etag), Ok(Some(held))) = (&etag, single(headers, &header::IF_NONE_MATCH))
        && etag_matches(&held, etag)
    {
        return not_modified(etag);
    }
    json_response(StatusCode::OK, reply.text, etag.as_deref())
}

/// `GET /api/testing/fleet`: one row per repository the data classes of this device release
/// (the export form of `testing-export/1`), from the snapshot the hub holds: asking does not start
/// the engine while the snapshot is good.
async fn fleet(
    State(st): State<Arc<Shared>>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    if let Some(response) = no_query(query.as_deref()) {
        return response;
    }
    let rt = st.rt();
    if !rt.store.has_engine() {
        return engine_unavailable(&rt);
    }
    let cached = match rt.store.current().await {
        Ok(cached) => cached,
        Err(error) => return engine_failure(error),
    };
    if let (Some(etag), Ok(Some(held))) = (&cached.etag, single(&headers, &header::IF_NONE_MATCH))
        && etag_matches(&held, etag)
    {
        return not_modified(etag);
    }
    json_response(StatusCode::OK, cached.text.clone(), cached.etag.as_deref())
}

/// The key of `?key=<key>`, the only query `repo` and `finding` take. A key is plain
/// (`engine::is_repo_key`), so nothing is decoded: an escaped character is no key.
pub(crate) fn repo_key(query: Option<&str>) -> Result<&str, &'static str> {
    let query = query.filter(|q| !q.is_empty()).ok_or("key is missing")?;
    let mut key = None;
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').ok_or("a query part has no value")?;
        if name != "key" {
            return Err("unknown query parameter");
        }
        if key.replace(value).is_some() {
            return Err("a query parameter is given twice");
        }
    }
    let key = key.ok_or("key is missing")?;
    if !is_repo_key(key) {
        return Err("key is not a key of the fleet");
    }
    Ok(key)
}

/// A document of one repository by its key. A key the engine finds no repository for is `404`; a
/// repository the data classes do not release is refused the same way, since the engine says both
/// with the same exit code.
async fn by_key(
    st: &Shared,
    query: Option<&str>,
    headers: &HeaderMap,
    method: Method,
    request: fn(&Ask, &str) -> Value,
) -> Response {
    let key = match repo_key(query) {
        Ok(key) => key,
        Err(why) => return error_body(StatusCode::BAD_REQUEST, "bad_query", why),
    };
    let rt = st.rt();
    let Some(engine) = &rt.engine else {
        return engine_unavailable(&rt);
    };
    match engine.call(method, Some(&request(&rt.ask, key))).await {
        Ok(reply) => document(headers, reply),
        // testing.nvim says "no such repository" and "not released" with exit 2 and no document
        Err(EngineError::Failed { code: Some(2), .. }) => error_body(
            StatusCode::NOT_FOUND,
            "not_found",
            "no released repository with this key",
        ),
        Err(error) => engine_failure(error),
    }
}

/// `GET /api/testing/repo?key=<key>`: one repository with its red cases.
async fn repo(
    State(st): State<Arc<Shared>>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    by_key(&st, query.as_deref(), &headers, Calls::Repo, Ask::repo).await
}

/// `GET /api/testing/finding?key=<key>`: the findings of the last run of one repository, in one
/// document (no finding is an empty list).
async fn finding(
    State(st): State<Arc<Shared>>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    by_key(
        &st,
        query.as_deref(),
        &headers,
        Calls::Finding,
        Ask::finding,
    )
    .await
}

/// `GET /api/testing/about`: what this build is, which engine it runs, and where the hub reads
/// from (as a word, never the path). Without an engine, or with one that does not answer, the app
/// is still told.
async fn about(State(st): State<Arc<Shared>>, RawQuery(query): RawQuery) -> Response {
    if let Some(response) = no_query(query.as_deref()) {
        return response;
    }
    let rt = st.rt();
    // An engine that is there but does not answer is told apart from no engine.
    let mut problem: Option<&'static str> = None;
    let engine = match &rt.engine {
        None => None,
        Some(engine) => match engine.call(Calls::Hello, None).await {
            Ok(reply) => match serde_json::from_str::<Value>(&reply.text) {
                // A `hello` without an `engine` object is an engine that broke the contract, not
                // a missing engine.
                Ok(doc) if doc["engine"].is_object() => Some(doc["engine"].clone()),
                Ok(_) => {
                    problem = Some("engine_protocol");
                    None
                }
                Err(_) => {
                    problem = Some("engine_failed");
                    None
                }
            },
            Err(error) => {
                problem = Some(about_problem(&error));
                None
            }
        },
    };
    let mut body = about_document(engine.as_ref(), problem);
    body["reads"] = json!({
        "state_dir": rt.origins.state_dir.map(crate::settings::Origin::word),
        "areas": rt.origins.areas.map(crate::settings::Origin::word),
    });
    body["ci"] = json!(rt.origins.ci.word());
    json_response(StatusCode::OK, body.to_string(), None)
}

/// A short, fixed word for why the engine did not answer `hello` (no text of the engine, no path).
fn about_problem(error: &EngineError) -> &'static str {
    match error {
        EngineError::Refused(_) => "engine_refused",
        EngineError::BadRequest(_) => "bad_request",
        EngineError::Busy => "engine_busy",
        EngineError::Timeout => "engine_timeout",
        EngineError::Spawn(_) => "engine_spawn",
        EngineError::Process(_) | EngineError::Failed { .. } | EngineError::OutcomeUnknown(_) => {
            "engine_failed"
        }
        EngineError::Protocol(_) => "engine_protocol",
    }
}

/// The document of `about`. `engine` is the `engine` object of the engine's `hello` (`version`,
/// `git`, `nvim`); only those three are taken over, whatever else the engine says. `None` when
/// there is no engine or it did not answer: then `engine_problem` says why.
pub(crate) fn about_document(engine: Option<&Value>, engine_problem: Option<&str>) -> Value {
    json!({
        "schema": 1,
        "kind": "hub.about",
        "app": IDENTITY.app,
        "version": crate::version(),
        "engine": engine.filter(|e| e.is_object()).map(|e| json!({
            "version": e["version"],
            "git": e["git"],
            "nvim": e["nvim"],
        })),
        "engine_problem": engine_problem,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_route_is_under_api_and_named_once() {
        let names: Vec<&str> = ROUTES.iter().map(|(n, _)| *n).collect();
        for name in &names {
            assert!(IDENTITY.route(name).starts_with("/api/testing/"), "{name}");
            assert_eq!(names.iter().filter(|n| *n == name).count(), 1, "{name}");
        }
        assert_eq!(
            names,
            [
                "fleet",
                "repo",
                "finding",
                "events",
                "about",
                "sheet-step",
                "sheet-step/sheets",
                "sheet-step/start",
                "sheet-step/command",
                "sheet-step/image",
                "ci-rerun",
                "run",
                "run/start",
                "run/task-sheets",
                "run/task",
            ]
        );
    }

    #[test]
    fn about_takes_only_the_three_fields_of_the_engine() {
        let engine = json!({"version": "1", "git": "abc", "nvim": "0.12.2", "root": "/home/x"});
        let doc = about_document(Some(&engine), None);
        assert_eq!(
            doc["engine"],
            json!({"version": "1", "git": "abc", "nvim": "0.12.2"})
        );
        assert_eq!(doc["app"], "testing-desktop");
        assert!(doc["engine_problem"].is_null());
        let none = about_document(None, Some("engine_failed"));
        assert!(none["engine"].is_null());
        assert_eq!(none["engine_problem"], "engine_failed");
        // Not an object: nothing is taken over.
        assert!(about_document(Some(&json!("x")), None)["engine"].is_null());
    }

    #[test]
    fn the_query_of_repo_is_one_plain_key() {
        assert_eq!(
            repo_key(Some("key=alpha.nvim-0123456789ab")),
            Ok("alpha.nvim-0123456789ab")
        );
        for bad in [
            None,
            Some(""),
            Some("key"),
            Some("key="),
            Some("id=alpha.nvim-0123456789ab"),
            Some("key=alpha.nvim-0123456789ab&key=alpha.nvim-0123456789ab"),
            Some("key=alpha.nvim-0123456789ab&x=1"),
            Some("key=alpha%2Envim-0123456789ab"),
            Some("key=..%2F..-0123456789ab"),
            Some("key=alpha.nvim"),
        ] {
            assert!(repo_key(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_etag_is_the_digest_of_the_contract_and_nothing_else() {
        assert_eq!(
            digest_of(r#"{"digest":"sha256:9ef3e271300fa214"}"#).as_deref(),
            Some("sha256:9ef3e271300fa214")
        );
        for bad in [
            r#"{"digest":"md5:abc"}"#,
            r#"{"digest":"sha256:"}"#,
            r#"{"digest":"sha256:xyz"}"#,
            r#"{"digest":"sha256:ab\"c"}"#,
            r#"{"digest":1}"#,
            r#"{}"#,
            "not json",
        ] {
            assert_eq!(digest_of(bad), None, "{bad}");
        }
    }
}
