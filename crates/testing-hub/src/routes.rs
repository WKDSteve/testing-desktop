//! The routes of the app, under `/api/testing/`. Each is under `/api/`, so the guard asked for a
//! session before a handler runs.
//!
//! [`ROUTES`] is the one list of them: the router is built from it and the security matrix reads
//! its names, so a route cannot be served without the matrix trying it without a session.

use std::sync::Arc;

use appkit_hub::{json_response, no_query, Core};
use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, MethodRouter};
use axum::Router;
use serde_json::{json, Value};

use crate::runtime::TestingRuntime;
use crate::IDENTITY;

/// The core of the hub with the runtime of this app.
pub type Shared = Core<TestingRuntime>;

/// How one route is answered: its method and handler.
pub type MakeRoute = fn() -> MethodRouter<Arc<Shared>>;

/// Every route of the app, by its name below `/api/testing/`.
pub const ROUTES: &[(&str, MakeRoute)] = &[("about", || get(about))];

/// The router of the app, built from [`ROUTES`] and from nothing else. Invariant: this stays a
/// pure fold over the table, with no extra `route` or `merge`, because [`crate::DATA_ROUTES`] and
/// the security matrix only know the routes of the table; a route added beside it would be served
/// without being tried there.
pub(crate) fn app_routes(_core: &Arc<Shared>) -> Router<Arc<Shared>> {
    ROUTES.iter().fold(Router::new(), |router, (name, make)| {
        router.route(&IDENTITY.route(name), make())
    })
}

/// `GET /api/testing/about`: what this build is and which engine it runs. There is no engine yet,
/// and the document says so rather than leaving the field out.
async fn about(State(_st): State<Arc<Shared>>, RawQuery(query): RawQuery) -> Response {
    if let Some(response) = no_query(query.as_deref()) {
        return response;
    }
    json_response(StatusCode::OK, about_document(None, None).to_string(), None)
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
        assert_eq!(names, ["about"]);
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
}
