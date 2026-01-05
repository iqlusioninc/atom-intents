//! WebSocket handlers for real-time updates

use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::models::WsMessage;
use crate::state::AppState;

type AppStateRef = Arc<RwLock<AppState>>;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppStateRef>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppStateRef) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to broadcast channel
    let mut rx = {
        let state = state.read().await;
        state.ws_broadcast.subscribe()
    };

    // Spawn task to forward broadcasts to this client
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            let json = match serde_json::to_string(&msg) {
                Ok(j) => j,
                Err(e) => {
                    error!("Failed to serialize message: {}", e);
                    continue;
                }
            };

            if sender.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages from client
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                    match ws_msg {
                        WsMessage::Ping => {
                            info!("Received ping from client");
                            // Pong is handled by broadcast
                            let state = state.read().await;
                            state.broadcast(WsMessage::Pong);
                        }
                        WsMessage::Subscribe { topics } => {
                            info!("Client subscribed to: {:?}", topics);
                            // In a full implementation, we'd filter broadcasts by topic
                        }
                        WsMessage::Unsubscribe { topics } => {
                            info!("Client unsubscribed from: {:?}", topics);
                        }
                        _ => {
                            // Client shouldn't send other message types
                        }
                    }
                }
            }
            Ok(Message::Ping(data)) => {
                // WebSocket protocol-level ping, handled automatically
                info!("Received WebSocket ping");
                let _ = data; // Would respond with pong
            }
            Ok(Message::Close(_)) => {
                info!("Client disconnected");
                break;
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    send_task.abort();
    info!("WebSocket connection closed");
}
