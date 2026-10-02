// photo-app/backend/src/routes/events.rs

use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue},
    response::sse::{Event, KeepAlive, Sse},
    response::IntoResponse,
};
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::AppState;

pub async fn stream_events(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let rx = state.tx_events.subscribe();

    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(event) => {
            let event_name = match &event {
                crate::WsMediaEvent::AssetReady { .. } => "asset_ready",
                crate::WsMediaEvent::AssetFailed { .. } => "asset_failed",
                crate::WsMediaEvent::AlbumUpdated { .. } => "album_updated",
                crate::WsMediaEvent::PeopleUpdated { .. } => "people_updated",
                crate::WsMediaEvent::AiCompleted { .. } => "ai_completed",
            };

            match serde_json::to_string(&event) {
                Ok(json) => {
                    let sse_event = Event::default().event(event_name).data(json);
                    Some(Ok::<Event, Infallible>(sse_event))
                }
                Err(_) => None,
            }
        }
        Err(_) => None,
    });

    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache, no-transform"));
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    headers.insert("X-Accel-Buffering", HeaderValue::from_static("no"));

    (
        headers,
        Sse::new(stream).keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("ping"),
        ),
    )
}