use futures_util::SinkExt;
use redis::RedisManager;
use tokio_tungstenite::tungstenite::Message;

use crate::{
    types::{WsMessage, WsResponse},
    user::User,
};
use std::collections::HashMap;

pub struct WsManager {
    pub users: HashMap<String, User>,
    pub subscriptions: HashMap<String, Vec<String>>,
    pub reverse_subscriptions: HashMap<String, Vec<String>>,
    pub redis_connection: RedisManager,
    api_key: String,
}

impl WsManager {
    pub async fn new(api_key: String) -> Self {
        Self {
            users: HashMap::new(),
            subscriptions: HashMap::new(),
            reverse_subscriptions: HashMap::new(),
            redis_connection: RedisManager::new()
                .await
                .expect("ws-stream: failed to connect to Redis"),
            api_key,
        }
    }

    pub fn add_user(&mut self, user: User) {
        self.users.insert(user.id.clone(), user);
    }

    pub fn remove_user(&mut self, id: &str) {
        self.users.remove(id);
        self.subscriptions.remove(id);

        for subscriptions in self.reverse_subscriptions.values_mut() {
            subscriptions.retain(|user_id| user_id != id);
        }
    }

    pub fn is_authenticated(&self, user_id: &str) -> bool {
        self.users
            .get(user_id)
            .map_or(false, |u| u.authenticated)
    }

    pub async fn authenticate(&mut self, user_id: &str, message: &WsMessage) -> bool {
        let key = message.params.first().map(|s| s.as_str()).unwrap_or("");
        if key == self.api_key {
            if let Some(user) = self.users.get_mut(user_id) {
                user.authenticated = true;
                let resp = serde_json::json!({
                    "method": "AUTH",
                    "result": "authenticated",
                    "id": message.id,
                });
                let _ = user
                    .ws_stream
                    .send(Message::Text(resp.to_string()))
                    .await;
            }
            true
        } else {
            if let Some(user) = self.users.get_mut(user_id) {
                let resp = serde_json::json!({
                    "method": "AUTH",
                    "error": "invalid api key",
                    "id": message.id,
                });
                let _ = user
                    .ws_stream
                    .send(Message::Text(resp.to_string()))
                    .await;
            }
            false
        }
    }

    pub async fn subscribe(&mut self, user_id: &str, message: WsMessage) {
        if message.method != "SUBSCRIBE" {
            return;
        }

        if !self.is_authenticated(user_id) {
            tracing::warn!(%user_id, "subscribe rejected: not authenticated");
            if let Some(user) = self.users.get_mut(user_id) {
                let resp = serde_json::json!({
                    "error": "authentication required",
                    "id": message.id,
                });
                let _ = user
                    .ws_stream
                    .send(Message::Text(resp.to_string()))
                    .await;
            }
            return;
        }

        let (subscription_type, asset_pair) = match message.parse_subscription() {
            Some(result) => result,
            None => {
                tracing::warn!(params = ?message.params, "invalid subscription format");
                return;
            }
        };
        let subscription_id = format!("{:?}.{:?}", subscription_type, asset_pair);

        self.subscriptions
            .entry(user_id.to_string())
            .or_default()
            .push(subscription_id.clone());

        let users = self
            .reverse_subscriptions
            .entry(subscription_id.clone())
            .or_default();

        if users.is_empty() {
            users.push(user_id.to_string());
            if let Err(e) = self.redis_connection.subscribe(&subscription_id).await {
                tracing::error!(%e, "failed to subscribe in redis");
            }
        } else {
            users.push(user_id.to_string());
        }
    }

    pub async fn unsubscribe(&mut self, user_id: &str, message: WsMessage) {
        if message.method != "UNSUBSCRIBE" {
            return;
        }

        if !self.is_authenticated(user_id) {
            tracing::warn!(%user_id, "unsubscribe rejected: not authenticated");
            return;
        }

        let (subscription_type, asset_pair) = match message.parse_subscription() {
            Some(result) => result,
            None => {
                tracing::warn!(params = ?message.params, "invalid unsubscription format");
                return;
            }
        };
        let subscription_id = format!("{:?}.{:?}", subscription_type, asset_pair);

        if let Some(subscriptions) = self.subscriptions.get_mut(user_id) {
            subscriptions.retain(|id| id != &subscription_id);
        }

        if let Some(users) = self.reverse_subscriptions.get_mut(&subscription_id) {
            users.retain(|id| id != user_id);

            if users.is_empty() {
                self.reverse_subscriptions.remove(&subscription_id);
                if let Err(e) = self.redis_connection.unsubscribe(&subscription_id).await {
                    tracing::error!(%e, "failed to unsubscribe in redis");
                }
            }
        }
    }

    pub async fn send_to_ws_stream(&mut self, message: String) {
        let ws_message: WsResponse = match serde_json::from_str(&message) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(%e, "failed to parse ws message from redis");
                return;
            }
        };

        if let Some(users) = self.reverse_subscriptions.get(&ws_message.stream) {
            for user_id in users.clone() {
                if let Some(user) = self.users.get_mut(&user_id) {
                    if let Err(e) = user
                        .ws_stream
                        .send(Message::Text(message.clone()))
                        .await
                    {
                        tracing::error!(user_id = %user_id, %e, "failed to send ws message");
                    }
                }
            }
        }
    }
}
