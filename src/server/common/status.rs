use super::ComponentStatus;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Generic status manager for any configuration type
pub struct StatusManager<C> {
    status: Arc<RwLock<ComponentStatus>>,
    config: Arc<RwLock<C>>,
}

impl<C: Clone> StatusManager<C> {
    pub fn new(config: C) -> Self {
        Self {
            status: Arc::new(RwLock::new(ComponentStatus::default())),
            config: Arc::new(RwLock::new(config)),
        }
    }

    pub async fn get_status(&self) -> ComponentStatus {
        let status = self.status.read().await;
        status.clone()
    }

    pub async fn set_status(&self, new_status: ComponentStatus) {
        let mut status = self.status.write().await;
        *status = new_status.clone();
        debug!("📊 Status updated to: {:?}", new_status);
    }

    pub async fn get_config(&self) -> C {
        let config = self.config.read().await;
        config.clone()
    }

    pub async fn update_config(&self, new_config: C) {
        let mut config = self.config.write().await;
        *config = new_config;
        debug!("🔧 Configuration updated");
    }
}
