use std::fmt;
use std::time::Duration;
use uuid::Uuid;

use fred::types::RedisConfig;
use fred::{clients::SubscriberClient, prelude::*};

/// Named Redis list queues used for inter-service communication.
pub enum RedisQueues {
    Orders,
    Users,
    Database,
}

impl fmt::Display for RedisQueues {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RedisQueues::Orders => write!(f, "orders"),
            RedisQueues::Users => write!(f, "users"),
            RedisQueues::Database => write!(f, "database"),
        }
    }
}

/// Errors produced by the Redis RPC layer.
#[derive(Debug)]
pub enum RpcError {
    Subscribe(RedisError),
    Push(RedisError),
    Timeout,
    ChannelClosed,
    ValueConversion(String),
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcError::Subscribe(e) => write!(f, "subscribe failed: {e}"),
            RpcError::Push(e) => write!(f, "push failed: {e}"),
            RpcError::Timeout => write!(f, "timed out waiting for response"),
            RpcError::ChannelClosed => write!(f, "subscriber channel closed"),
            RpcError::ValueConversion(e) => write!(f, "value conversion failed: {e}"),
        }
    }
}

impl std::error::Error for RpcError {}

const RPC_TIMEOUT: Duration = Duration::from_secs(5);

pub struct RedisManager {
    pub client: RedisClient,
    pub publisher: RedisClient,
    pub subscriber: SubscriberClient,
}

impl RedisManager {
    pub async fn new() -> Result<Self, RedisError> {
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");

        let config = RedisConfig::from_url(&redis_url)
            .expect("Failed to create redis config from url");

        let client = Builder::from_config(config.clone()).build()?;
        let publisher = Builder::from_config(config.clone()).build()?;
        let subscriber = Builder::from_config(config).build_subscriber_client()?;

        client.init().await?;
        publisher.init().await?;
        subscriber.init().await?;

        Ok(Self {
            client,
            publisher,
            subscriber,
        })
    }

    pub async fn push(&self, key: &str, value: String) -> Result<(), RedisError> {
        self.client.lpush(key, value).await
    }

    pub async fn pop(
        &self,
        key: &str,
        count: Option<usize>,
    ) -> Result<Vec<RedisValue>, RedisError> {
        self.client.rpop(key, count).await
    }

    /// Remove exchange queue list keys. Intended for integration tests or controlled resets.
    pub async fn reset_exchange_queues(&self) -> Result<(), RedisError> {
        for key in ["orders", "users", "database"] {
            let _: () = self.client.del(key).await?;
        }
        Ok(())
    }

    /// Returns `Ok` if Redis responds to PING (used for readiness checks).
    pub async fn ping(&self) -> Result<(), RedisError> {
        let _: () = self.client.ping().await?;
        Ok(())
    }

    pub async fn publish(&self, channel: &str, value: String) -> Result<(), RedisError> {
        self.publisher.publish(channel, value).await
    }

    pub async fn subscribe(&self, channel: &str) -> Result<(), RedisError> {
        self.subscriber.subscribe(channel).await
    }

    pub async fn unsubscribe(&self, channel: &str) -> Result<(), RedisError> {
        self.subscriber.unsubscribe(channel).await
    }

    /// Push a message to a queue and wait for the engine to publish a response
    /// on a dedicated per-request channel, with a timeout.
    pub async fn push_and_wait_for_subscriber(
        &self,
        key: String,
        value: String,
        channel: Uuid,
    ) -> Result<String, RpcError> {
        let channel = channel.to_string();

        self.subscribe(&channel)
            .await
            .map_err(RpcError::Subscribe)?;

        if let Err(e) = self.push(&key, value).await {
            let _ = self.unsubscribe(&channel).await;
            return Err(RpcError::Push(e));
        }

        let mut message_stream = self.subscriber.message_rx();

        let result = tokio::time::timeout(RPC_TIMEOUT, async {
            loop {
                match message_stream.recv().await {
                    Ok(message) => {
                        if message.channel.to_string() != channel {
                            continue;
                        }
                        let published_message = message
                            .value
                            .convert::<String>()
                            .map_err(|e| RpcError::ValueConversion(e.to_string()))?;
                        return Ok::<String, RpcError>(published_message);
                    }
                    Err(_) => return Err(RpcError::ChannelClosed),
                }
            }
        })
        .await;

        let _ = self.unsubscribe(&channel).await;

        match result {
            Ok(inner) => inner,
            Err(_) => Err(RpcError::Timeout),
        }
    }
}
