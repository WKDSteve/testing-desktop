//! The route that follows the state folder: the event stream.

use std::sync::Arc;

use appkit_hub::streams::{sse_response, too_many_streams};
use appkit_hub::{bearer, no_query};
use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::header::HeaderMap;
use axum::response::Response;
use serde_json::json;
use tokio::sync::{broadcast, mpsc};

use crate::answer::{engine_failure, engine_unavailable};
use crate::routes::Shared;
use crate::store::SHAPE;

/// `GET /api/testing/events`: server-sent events. A `hello` first (with where the stream stands),
/// then one `fleet` event per change. An event is a hint: the client reads again. There is no
/// replay: a client that connects reads the snapshot first. A client that was too slow for the
/// buffer gets `resync` and reads everything again.
///
/// The lines of a stepped run of a sheet come on the same stream, as `sheet-step` events with the
/// number of the run and of the line (`sheetstep`): a client reads the state of the run first and
/// takes only the lines after it; after a `resync` it reads the state again.
pub(crate) async fn events(
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
    // The guard has checked the session; the token is the key of the count.
    let token = bearer(&headers).unwrap_or_default();
    let Some(guard) = rt
        .streams
        .acquire(&token, st.limits.max_streams_per_session)
    else {
        return too_many_streams();
    };
    // Start from a known state: the snapshot is held before the stream is.
    if let Err(error) = rt.store.current().await {
        return engine_failure(error);
    }
    let (mut changes, seq, rev) = rt.store.subscribe();
    let mut steps = rt.sheet_step.subscribe();
    let (tx, rx) = mpsc::channel::<Bytes>(16);
    let heartbeat = st.limits.heartbeat;
    let instance = st.instance.clone();
    let shared = Arc::clone(&st);
    tokio::spawn(async move {
        let _guard = guard;
        let hello = format!(
            "event: hello\ndata: {}\n\n",
            json!({ "rev": rev, "seq": seq, "instance": instance })
        );
        // The hello too: the session may have been closed while the snapshot was read.
        if !shared.sessions.check(&token) {
            return;
        }
        if tx.send(Bytes::from(hello)).await.is_err() {
            return;
        }
        let mut ticker =
            tokio::time::interval_at(tokio::time::Instant::now() + heartbeat, heartbeat);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let message = tokio::select! {
                change = changes.recv() => match change {
                    Ok(c) => c.to_sse(SHAPE.event),
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        "event: resync\ndata: {}\n\n".to_owned()
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                line = steps.recv() => match line {
                    Ok(line) => line.to_string(),
                    // Too slow for the lines of the run: the client reads its state again.
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        "event: resync\ndata: {}\n\n".to_owned()
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                _ = ticker.tick() => ": ping\n\n".to_owned(),
                () = tx.closed() => return,
            };
            // A session that was closed since the stream was opened takes its streams with it.
            if !shared.sessions.check(&token) {
                return;
            }
            if tx.send(Bytes::from(message)).await.is_err() {
                return;
            }
        }
    });
    sse_response(rx)
}
