use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use warp::Filter;
use warp::ws::{Message, WebSocket};
use futures::{StreamExt, SinkExt};
use tokio::sync::mpsc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

type Users = Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Message>>>>;

#[derive(Deserialize, Serialize)]
struct MessagePayload {
    recipient_id: String,
    message: Value, // Allows for nested JSON objects
}

#[tokio::main]
async fn main() {
    let users = Users::default();

    let users_filter = warp::any().map(move || Arc::clone(&users));

    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(warp::query::<HashMap<String, String>>())
        .and(users_filter)
        .map(|ws: warp::ws::Ws, query: HashMap<String, String>, users: Users| {
            let user_id = query.get("id").cloned().unwrap_or_else(|| "".to_string());
            ws.on_upgrade(move |socket| handle_connection(socket, user_id, users))
        });

    println!("WebSocket server is running on ws://0.0.0.0:3030/ws");

    warp::serve(ws_route).run(([0, 0, 0, 0], 3030)).await;
}

async fn handle_connection(ws: WebSocket, user_id: String, users: Users) {
    println!("User connected with ID: {}", user_id);

    let (mut user_ws_tx, mut user_ws_rx) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel();

    users.lock().unwrap().insert(user_id.clone(), tx);

    tokio::task::spawn(async move {
        while let Some(result) = rx.recv().await {
            let _ = user_ws_tx.send(result).await;
        }
    });

    while let Some(result) = user_ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                println!("Error receiving message for user {}: {:?}", user_id, e);
                break;
            }
        };

        if msg.is_text() {
            let payload: Result<MessagePayload, _> = serde_json::from_str(msg.to_str().unwrap());
            if let Ok(payload) = payload {
                println!(
                    "Received message from user {}: {:?}",
                    user_id, payload.message
                );
                if let Some(recipient_tx) = users.lock().unwrap().get(&payload.recipient_id) {
                    println!(
                        "Forwarding message from user {} to user {}",
                        user_id, payload.recipient_id
                    );
                    let _ = recipient_tx.send(Message::text(serde_json::to_string(&payload.message).unwrap()));
                } else {
                    println!(
                        "Recipient with ID {} not found. Message from user {} was not delivered.",
                        payload.recipient_id, user_id
                    );
                }
            } else {
                println!("Failed to parse message from user {}: {:?}", user_id, msg);
            }
        }
    }

    users.lock().unwrap().remove(&user_id);
    println!("User disconnected: {}", user_id);
}
