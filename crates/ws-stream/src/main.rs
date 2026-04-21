use fred::prelude::*;
use futures_util::{SinkExt, StreamExt};
use std::io::Error;
use std::time::Duration;
use std::{sync::Arc, thread};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::{
    protocol::frame::coding::CloseCode, protocol::CloseFrame, Error as WsError, Message,
};
use types::WsMessage;

pub mod types;
pub mod user;
pub mod ws_manager;

use user::User;
use ws_manager::WsManager;

const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
const CLOSE_AUTH_FAILED: u16 = 4001;

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let addr = std::env::var("WS_STREAM_URL").expect("WS_STREAM_URL must be set");
    let api_key = std::env::var("API_KEY").expect("API_KEY must be set");

    let listener = TcpListener::bind(&addr).await?;
    let ws_manager = Arc::new(Mutex::new(WsManager::new(api_key).await));

    let ws_manager_clone = ws_manager.clone();
    thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(process_redis_message(ws_manager_clone));
    });

    tracing::info!(%addr, "websocket server listening");

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream, ws_manager.clone()));
    }

    Ok(())
}

async fn accept_connection(stream: TcpStream, ws_manager: Arc<Mutex<WsManager>>) {
    let user_addr = match stream.peer_addr() {
        Ok(addr) => addr.to_string(),
        Err(e) => {
            tracing::error!(%e, "failed to get peer address");
            return;
        }
    };

    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            tracing::error!(user_addr = %user_addr, %e, "websocket handshake failed");
            return;
        }
    };

    let (write, mut read) = ws_stream.split();
    let user = User::new(user_addr.clone(), write);

    {
        let mut manager = ws_manager.lock().await;
        manager.add_user(user);
    }

    // Require AUTH as first message within timeout
    let authenticated = match tokio::time::timeout(AUTH_TIMEOUT, read.next()).await {
        Ok(Some(Ok(msg))) if msg.is_text() => {
            if let Ok(text) = msg.to_text() {
                if let Ok(data) = serde_json::from_str::<WsMessage>(text) {
                    if data.method == "AUTH" {
                        let mut manager = ws_manager.lock().await;
                        manager.authenticate(&user_addr, &data).await
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        }
        _ => false,
    };

    if !authenticated {
        tracing::warn!(%user_addr, "auth failed or timed out, closing connection");
        let mut manager = ws_manager.lock().await;
        if let Some(user) = manager.users.get_mut(&user_addr) {
            let close_frame = CloseFrame {
                code: CloseCode::from(CLOSE_AUTH_FAILED),
                reason: "authentication failed or timed out".into(),
            };
            let _ = user.ws_stream.send(Message::Close(Some(close_frame))).await;
        }
        manager.remove_user(&user_addr);
        return;
    }

    tracing::info!(%user_addr, "client authenticated");

    while let Some(msg_result) = read.next().await {
        match msg_result {
            Ok(msg) => {
                if msg.is_text() {
                    if let Ok(text) = msg.to_text() {
                        if let Ok(data) = serde_json::from_str::<WsMessage>(text) {
                            process_data(data, &user_addr, ws_manager.clone()).await;
                        }
                    }
                } else if msg.is_close() {
                    let mut manager = ws_manager.lock().await;
                    manager.remove_user(&user_addr);
                    break;
                }
            }
            Err(e) => {
                match e {
                    WsError::Protocol(ref protocol_err) => {
                        tracing::error!(
                            user_addr = %user_addr,
                            ?protocol_err,
                            "websocket protocol error"
                        );
                    }
                    WsError::ConnectionClosed | WsError::AlreadyClosed => {
                        tracing::info!(%user_addr, "websocket closed");
                    }
                    _ => {
                        tracing::error!(user_addr = %user_addr, %e, "websocket error");
                    }
                }
                let mut manager = ws_manager.lock().await;
                manager.remove_user(&user_addr);
                break;
            }
        }
    }
}

async fn process_data(data: WsMessage, user_addr: &str, ws_manager: Arc<Mutex<WsManager>>) {
    let mut manager = ws_manager.lock().await;

    match data.method.as_str() {
        "SUBSCRIBE" => manager.subscribe(user_addr, data).await,
        "UNSUBSCRIBE" => manager.unsubscribe(user_addr, data).await,
        _ => {}
    }
}

async fn process_redis_message(ws_manager: Arc<Mutex<WsManager>>) {
    let mut message_stream;

    {
        let manager = ws_manager.lock().await;
        message_stream = manager.redis_connection.subscriber.message_rx();
    }

    while let Ok(message) = message_stream.recv().await {
        let publisher_message = match message.value {
            RedisValue::String(s) => s.to_string(),
            _ => continue,
        };

        let mut manager = ws_manager.lock().await;
        manager.send_to_ws_stream(publisher_message).await;
    }
}
