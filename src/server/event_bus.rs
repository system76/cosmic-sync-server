use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

#[async_trait]
pub trait EventBus: Send + Sync {
    async fn publish(&self, routing_key: &str, payload: Vec<u8>) -> anyhow::Result<()>;
    async fn subscribe(&self, binding_key: &str) -> anyhow::Result<ReceiverStream<Vec<u8>>>;
}

// NoopEventBus: placeholder until a real MQ is wired (e.g., RabbitMQ via lapin)
#[derive(Clone, Default)]
pub struct NoopEventBus;

#[async_trait]
impl EventBus for NoopEventBus {
    async fn publish(&self, _routing_key: &str, _payload: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn subscribe(&self, _binding_key: &str) -> anyhow::Result<ReceiverStream<Vec<u8>>> {
        let (_tx, rx) = mpsc::channel::<Vec<u8>>(1);
        Ok(ReceiverStream::new(rx))
    }
}

// RabbitMQ implementation
use lapin::{options::*, types::FieldTable, BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind};
use tokio_stream::StreamExt;
use std::sync::Arc;

#[derive(Clone)]
pub struct RabbitMqEventBus {
	channel: Arc<Channel>,
	exchange: String,
	queue_prefix: String,
	prefetch: u16,
}

impl RabbitMqEventBus {
	pub async fn connect(url: &str, exchange: &str, queue_prefix: &str, prefetch: u16, durable: bool) -> anyhow::Result<Self> {
		let conn = Connection::connect(url, ConnectionProperties::default()).await?;
		let channel = conn.create_channel().await?;

		channel.exchange_declare(
			exchange,
			ExchangeKind::Topic,
			ExchangeDeclareOptions { passive: false, durable, auto_delete: false, internal: false, nowait: false },
			FieldTable::default(),
		).await?;

		channel.basic_qos(prefetch as u16, BasicQosOptions { global: false }).await?;

		Ok(Self { channel: Arc::new(channel), exchange: exchange.to_string(), queue_prefix: queue_prefix.to_string(), prefetch })
	}

	pub fn channel(&self) -> Channel {
		self.channel.as_ref().clone()
	}
}

#[async_trait]
impl EventBus for RabbitMqEventBus {
	async fn publish(&self, routing_key: &str, payload: Vec<u8>) -> anyhow::Result<()> {
		self.channel
			.basic_publish(
				&self.exchange,
				routing_key,
				BasicPublishOptions { mandatory: false, immediate: false },
				&payload,
				BasicProperties::default(),
			)
			.await?
			.await?; // publisher confirm
		Ok(())
	}

	async fn subscribe(&self, binding_key: &str) -> anyhow::Result<ReceiverStream<Vec<u8>>> {
		let qname = format!("{}.{}", self.queue_prefix, nanoid::nanoid!(6));
		self.channel.queue_declare(
			&qname,
			QueueDeclareOptions { passive: false, durable: true, exclusive: true, auto_delete: true, nowait: false },
			FieldTable::default(),
		).await?;
		self.channel.queue_bind(
			&qname,
			&self.exchange,
			binding_key,
			QueueBindOptions::default(),
			FieldTable::default(),
		).await?;

		let (tx, rx) = mpsc::channel::<Vec<u8>>(self.prefetch as usize);
		let mut consumer = self.channel
			.basic_consume(
				&qname,
				"cosmic-consumer",
				BasicConsumeOptions { no_local: false, no_ack: false, exclusive: false, nowait: false },
				FieldTable::default(),
			)
			.await?;

		let channel = self.channel.clone();
		tokio::spawn(async move {
			while let Some(delivery) = consumer.next().await {
				if let Ok(delivery) = delivery {
					let _ = tx.send(delivery.data.clone()).await;
					let _ = channel.basic_ack(delivery.delivery_tag, BasicAckOptions::default()).await;
				}
			}
		});

		Ok(ReceiverStream::new(rx))
	}
}


