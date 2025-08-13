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


