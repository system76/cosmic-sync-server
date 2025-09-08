use tokio::signal;
use tokio_stream::StreamExt;
use tracing::{debug, error, info, warn};

use cosmic_sync_server::{MessageBrokerConfig, RabbitMqEventBus};

use lapin::{
    options::*,
    types::{AMQPValue, FieldTable},
    BasicProperties, ExchangeKind,
};
#[cfg(not(feature = "redis-cache"))]
use once_cell::sync::Lazy;
#[cfg(not(feature = "redis-cache"))]
use std::collections::HashSet;
#[cfg(not(feature = "redis-cache"))]
use std::sync::Mutex;

#[cfg(feature = "redis-cache")]
use redis::{aio::ConnectionManager, Client as RedisClient};
#[cfg(feature = "redis-cache")]
use tokio::sync::OnceCell;

#[cfg(not(feature = "redis-cache"))]
static SEEN_IDS: Lazy<Mutex<HashSet<String>>> =
    Lazy::new(|| Mutex::new(HashSet::with_capacity(4096)));

fn sanitize_binding_key(key: &str) -> String {
    key.replace('*', "star")
        .replace('#', "hash")
        .replace('.', "_")
}

fn payload_id(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
            if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
                return id.to_string();
            }
        }
    }
    format!("h:{}", bytes.len())
}

#[cfg(not(feature = "redis-cache"))]
async fn mark_seen_id_async(id: &str) -> bool {
    let mut seen = SEEN_IDS.lock().unwrap();
    seen.insert(id.to_string())
}

#[cfg(feature = "redis-cache")]
static REDIS_MANAGER: OnceCell<ConnectionManager> = OnceCell::const_new();

#[cfg(feature = "redis-cache")]
async fn get_redis_manager() -> Option<ConnectionManager> {
    let url = match std::env::var("REDIS_URL") {
        Ok(u) => u,
        Err(_) => return None,
    };
    let mgr_ref = REDIS_MANAGER
        .get_or_init(|| async move {
            match RedisClient::open(url.clone()) {
                Ok(client) => match ConnectionManager::new(client).await {
                    Ok(mgr) => mgr,
                    Err(e) => {
                        error!("Failed to create Redis connection manager: {}", e);
                        panic!("redis connection failed")
                    }
                },
                Err(e) => {
                    error!("Failed to create Redis client: {}", e);
                    panic!("redis client failed")
                }
            }
        })
        .await;
    Some(mgr_ref.clone())
}

#[cfg(feature = "redis-cache")]
async fn mark_seen_id_async(id: &str) -> bool {
    if let Some(mut manager) = get_redis_manager().await {
        let ttl_secs: i64 = std::env::var("IDEMPOTENCY_TTL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3600);
        let res: redis::RedisResult<Option<String>> = redis::cmd("SET")
            .arg(id)
            .arg("1")
            .arg("NX")
            .arg("EX")
            .arg(ttl_secs)
            .query_async(&mut manager)
            .await;
        match res {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(e) => {
                warn!("Redis SET NX EX failed (fallback accept): {}", e);
                false
            }
        }
    } else {
        // No REDIS_URL set → fallback to in-memory tracker
        #[cfg(not(feature = "redis-cache"))]
        {
            let mut seen = SEEN_IDS.lock().unwrap();
            return seen.insert(id.to_string());
        }
        #[allow(unreachable_code)]
        {
            false
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cfg = MessageBrokerConfig::load();
    if !cfg.enabled {
        info!("Message broker disabled. Exiting consumer.");
        return Ok(());
    }

    let bus = match RabbitMqEventBus::connect(
        &cfg.url,
        &cfg.exchange,
        &cfg.queue_prefix,
        cfg.prefetch,
        cfg.durable,
    )
    .await
    {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to connect to RabbitMQ: {}", e);
            return Err(e);
        }
    };
    let channel = bus.channel();

    // Declare DLX exchange
    let dlx_exchange = format!("{}.dlx", cfg.exchange);
    channel
        .exchange_declare(
            &dlx_exchange,
            ExchangeKind::Topic,
            ExchangeDeclareOptions {
                passive: false,
                durable: true,
                auto_delete: false,
                internal: false,
                nowait: false,
            },
            FieldTable::default(),
        )
        .await?;

    let retry_ttl_ms: u32 = std::env::var("RETRY_TTL_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5_000);
    let max_retries: u32 = std::env::var("MAX_RETRIES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let keys = vec![
        "file.*.#",
        "version.*.#",
        "device.*.#",
        "watcher.*.update.#",
    ]; // default bindings

    // For each binding key, create durable queues with DLX and retry and start consumers
    let mut consumers = Vec::new();
    for key in keys {
        let qbase = format!(
            "{}.{}.{}",
            cfg.queue_prefix,
            "consumer",
            sanitize_binding_key(key)
        );
        let qmain = qbase.clone();
        let qretry = format!("{}.retry", qbase);
        let qdlq = format!("{}.dlq", qbase);

        // Main queue -> DLX on failure
        let mut main_args = FieldTable::default();
        main_args.insert(
            "x-dead-letter-exchange".into(),
            AMQPValue::LongString(dlx_exchange.clone().into()),
        );
        channel
            .queue_declare(
                &qmain,
                QueueDeclareOptions {
                    passive: false,
                    durable: true,
                    exclusive: false,
                    auto_delete: false,
                    nowait: false,
                },
                main_args,
            )
            .await?;
        channel
            .queue_bind(
                &qmain,
                &cfg.exchange,
                key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        // Retry queue -> back to main exchange after TTL
        let mut retry_args = FieldTable::default();
        retry_args.insert(
            "x-dead-letter-exchange".into(),
            AMQPValue::LongString(cfg.exchange.clone().into()),
        );
        retry_args.insert("x-message-ttl".into(), AMQPValue::LongUInt(retry_ttl_ms));
        channel
            .queue_declare(
                &qretry,
                QueueDeclareOptions {
                    passive: false,
                    durable: true,
                    exclusive: false,
                    auto_delete: false,
                    nowait: false,
                },
                retry_args,
            )
            .await?;
        channel
            .queue_bind(
                &qretry,
                &cfg.exchange,
                key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        // DLQ bound to DLX
        channel
            .queue_declare(
                &qdlq,
                QueueDeclareOptions {
                    passive: false,
                    durable: true,
                    exclusive: false,
                    auto_delete: false,
                    nowait: false,
                },
                FieldTable::default(),
            )
            .await?;
        channel
            .queue_bind(
                &qdlq,
                &dlx_exchange,
                "#",
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        // Start consumer on main queue
        let mut stream = channel
            .basic_consume(
                &qmain,
                "cosmic-consumer",
                BasicConsumeOptions {
                    no_local: false,
                    no_ack: false,
                    exclusive: false,
                    nowait: false,
                },
                FieldTable::default(),
            )
            .await?;

        let channel_clone = channel.clone();
        let cfg_exchange = cfg.exchange.clone();
        let dlx_ex = dlx_exchange.clone();
        consumers.push(tokio::spawn(async move {
            while let Some(delivery) = stream.next().await {
                match delivery {
                    Ok(delivery) => {
                        let body = delivery.data.clone();
                        let blen = body.len();
                        let id = payload_id(&body);
                        let is_new = mark_seen_id_async(&id).await;
                        if !is_new {
                            debug!("Duplicate message skipped: {} ({} bytes)", id, blen);
                            let _ = channel_clone
                                .basic_ack(delivery.delivery_tag, BasicAckOptions::default())
                                .await;
                            continue;
                        }

                        // Simple handler: try parse JSON, if parse fails, route to retry/dlq with attempts
                        let mut attempts = 0u32;
                        if let Some(headers) = delivery.properties.headers().as_ref() {
                            if let Some(AMQPValue::LongInt(n)) =
                                headers.inner().get("x-retry-count")
                            {
                                attempts = (*n).max(0) as u32;
                            }
                            if let Some(AMQPValue::LongUInt(n)) =
                                headers.inner().get("x-retry-count")
                            {
                                attempts = *n as u32;
                            }
                        }

                        let try_parse = serde_json::from_slice::<serde_json::Value>(&body).is_ok();
                        if try_parse {
                            let _ = channel_clone
                                .basic_ack(delivery.delivery_tag, BasicAckOptions::default())
                                .await;
                        } else {
                            if attempts + 1 >= max_retries {
                                // Send to DLQ (publish to DLX exchange)
                                let props = BasicProperties::default().with_headers({
                                    let mut t = FieldTable::default();
                                    t.insert(
                                        "x-retry-count".into(),
                                        AMQPValue::LongUInt(attempts + 1),
                                    );
                                    t
                                });
                                let _ = channel_clone
                                    .basic_publish(
                                        &dlx_ex,
                                        delivery.routing_key.as_str(),
                                        BasicPublishOptions::default(),
                                        &body,
                                        props,
                                    )
                                    .await;
                                let _ = channel_clone
                                    .basic_ack(delivery.delivery_tag, BasicAckOptions::default())
                                    .await;
                            } else {
                                // Publish to retry with same routing key
                                let props = BasicProperties::default().with_headers({
                                    let mut t = FieldTable::default();
                                    t.insert(
                                        "x-retry-count".into(),
                                        AMQPValue::LongUInt(attempts + 1),
                                    );
                                    t
                                });
                                let _ = channel_clone
                                    .basic_publish(
                                        &cfg_exchange,
                                        delivery.routing_key.as_str(),
                                        BasicPublishOptions::default(),
                                        &body,
                                        props,
                                    )
                                    .await;
                                let _ = channel_clone
                                    .basic_ack(delivery.delivery_tag, BasicAckOptions::default())
                                    .await;
                            }
                        }
                    }
                    Err(e) => warn!("Consume error: {}", e),
                }
            }
        }));
    }

    // Wait for Ctrl+C and then stop
    signal::ctrl_c().await?;
    info!("Consumer exiting on Ctrl+C");
    for t in consumers {
        let _ = t.abort();
    }
    Ok(())
}
