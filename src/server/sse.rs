//! Server-sent events: `hello` first, then every `LiveEvent`, with keepalives.

use super::AppState;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream, StreamExt};
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;

pub async fn events(
    State(app): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let st = app.state.load();
    let hello = Event::default().event("hello").data(
        serde_json::json!({ "seq": app.seq.load(std::sync::atomic::Ordering::SeqCst), "generation": st.generation, "scanned_at": st.last_scan_at.map(|t| t.to_string()) })
            .to_string(),
    );
    let rx = app.events.subscribe();
    let live = BroadcastStream::new(rx).map(|item| match item {
        Ok(ev) => {
            Ok(Event::default().id(ev.seq.to_string()).event(ev.name).data(ev.payload.to_string()))
        }
        Err(_) => Ok(Event::default().event("state.resync").data("{}")),
    });
    let stream = stream::once(async move { Ok::<Event, Infallible>(hello) }).chain(live);
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)).text("ping"))
}
