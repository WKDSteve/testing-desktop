//! The route that follows the state folder: the event stream.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use appkit_hub::{bearer, error_body, no_query};
use axum::body::{Body, Bytes};
use axum::extract::{RawQuery, State};
use axum::http::header::{self, HeaderMap, HeaderValue};
use axum::http::StatusCode;
use axum::response::Response;
use futures_core::Stream;
use serde_json::json;
use tokio::sync::{broadcast, mpsc};

use crate::answer::{engine_failure, engine_unavailable};
use crate::routes::Shared;
use crate::store::SHAPE;

/// The body of an event stream: what the task of the connection sends.
struct EventStream {
    rx: mpsc::Receiver<Bytes>,
}

impl Stream for EventStream {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx).map(|item| item.map(Ok))
    }
}

/// `GET /api/testing/events`: server-sent events. A `hello` first (with where the stream stands),
/// then one `fleet` event per change. An event is a hint: the client reads again. There is no
/// replay: a client that connects reads the snapshot first. A client that was too slow for the
/// buffer gets `resync` and reads everything again.
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
        return error_body(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_streams",
            "this session has too many open event streams",
        );
    };
    // Start from a known state: the snapshot is held before the stream is.
    if let Err(error) = rt.store.current().await {
        return engine_failure(error);
    }
    let (mut changes, seq, rev) = rt.store.subscribe();
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
    let mut response = Response::new(Body::from_stream(EventStream { rx }));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    response
}
