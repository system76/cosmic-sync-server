use tracing::{debug, error, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use crate::server::app_state::AppState;

/// Connection handler manages client connections
pub struct ConnectionHandler {
    app_state: Option<Arc<AppState>>,
}

impl Default for ConnectionHandler {
    fn default() -> Self {
        Self {
            app_state: None,
        }
    }
}

impl ConnectionHandler {
    /// Create a new connection handler
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Set the application state
    pub fn set_app_state(&mut self, app_state: Arc<AppState>) {
        self.app_state = Some(app_state);
    }

    /// Handle client disconnection
    pub async fn handle_disconnect(&self, client_id: &str) {
        info!("Client disconnected: {}", client_id);
        
        // process only if app_state is set
        if let Some(app_state) = &self.app_state {
            // update client connection status
            if let Some(client_store) = app_state.get_client_store() {
                client_store.set_client_connected(client_id, false).await;
            }
            
            // handle unsubscription
            if let Err(e) = app_state
                .notification_manager
                .unregister_file_update_subscriber(client_id)
                .await
            {
                warn!("Failed to unregister file update subscriber: {:?}", e);
            }
        }
    }

    /// Handle client reconnection attempts
    pub async fn handle_reconnect(&self, client_id: &str, max_attempts: usize) -> bool {
        let mut attempts = 0;
        
        while attempts < max_attempts {
            attempts += 1;
            info!("Attempting to reconnect client {} (attempt {}/{})", client_id, attempts, max_attempts);
            
            // backoff wait time increases exponentially
            let wait_time = Duration::from_secs(2u64.pow(attempts as u32));
            sleep(wait_time).await;
            
            // try to reconnect
            if self.try_reconnect(client_id).await {
                info!("Successfully reconnected client: {}", client_id);
                return true;
            }
        }
        
        error!("Failed to reconnect client after {} attempts: {}", max_attempts, client_id);
        false
    }
    
    /// Try to reconnect a specific client
    async fn try_reconnect(&self, client_id: &str) -> bool {
        // process only if app_state is set
        if let Some(app_state) = &self.app_state {
            // actual reconnection logic is handled by the client side
            // the server side only updates the client status
            if let Some(client_store) = app_state.get_client_store() {
                client_store.set_client_connected(client_id, true).await;
                return true;
            }
        }
        false
    }
    
    /// Create a heartbeat channel for client
    pub async fn create_heartbeat_channel(&self, client_id: &str) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel(16);
        
        // start periodic heartbeat transmission
        let client_id = client_id.to_string();
        tokio::spawn(async move {
            let interval = Duration::from_secs(30); // send heartbeat every 30 seconds
            loop {
                sleep(interval).await;
                if tx.send(()).await.is_err() {
                    // consider the client disconnected if the channel is closed
                    debug!("Heartbeat channel closed for client: {}", client_id);
                    break;
                }
            }
        });
        
        rx
    }
}