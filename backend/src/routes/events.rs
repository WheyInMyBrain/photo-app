// photo-app/backend/src/routes/events.rs

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    response::IntoResponse,
};
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tracing::warn;

use crate::AppState;

pub async fn stream_events(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let rx = state.tx_events.subscribe();

    // Map the broadcast channel to an SSE stream without terminating on lag
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
                Err(err) => {
                    warn!("Failed to serialize SSE media event: {err}");
                    // Skip malformed items; do NOT terminate stream
                    None
                }
            }
        }
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(skipped)) => {
            warn!("Client lagged behind on SSE stream, skipped {skipped} messages");
            // Crucial: return None here in filter_map to skip the dropped message,
            // but the stream stays ALIVE for subsequent broadcast events!
            None
        }
    });

    // Let Axum's Sse builder set the required SSE headers and keepalive pings
    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("ping"),
        )
}