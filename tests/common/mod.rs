// Common test helpers for integration tests (English only in code)

use cosmic_sync_server::server::app_state::AppState;
use cosmic_sync_server::storage::memory::MemoryStorage;
use std::sync::Arc;

pub async fn app_state_with_memory() -> Arc<AppState> {
    let storage = Arc::new(MemoryStorage::new());
    let server_cfg = cosmic_sync_server::config::settings::ServerConfig::default();
    AppState::new_with_storage_and_server_config(storage, &server_cfg)
        .await
        .expect("Failed to build AppState")
        .into()
}
