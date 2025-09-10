use crate::auth::oauth::OAuthService;
use crate::config::settings::{Config, ServerConfig};
use crate::error::AppError;
use crate::server::event_bus::RabbitMqEventBus;
use crate::server::event_bus::{EventBus, NoopEventBus};
use crate::server::notification_manager::NotificationManager;
use crate::services::device_service::DeviceService;
use crate::services::encryption_service::EncryptionService;
use crate::services::file_service::FileService;
use crate::services::version_service::{VersionService, VersionServiceImpl};
use crate::storage::mysql::MySqlStorage;
use crate::storage::mysql_watcher::MySqlWatcherExt;
use crate::storage::{memory::MemoryStorage, FileStorage, Storage};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::server::connection_cleanup::{
    create_default_cleanup_scheduler, create_default_stats_reporter,
};
use crate::server::connection_handler::ConnectionHandler;
use crate::server::connection_tracker::ConnectionTracker;
use dashmap::DashMap;
use once_cell::sync::OnceCell;

/// Structure to store authentication session information
#[derive(Debug, Clone)]
pub struct AuthSession {
    // Session identification
    pub device_hash: String,
    pub client_id: String,

    // Authentication state
    pub auth_token: Option<String>,
    pub account_hash: Option<String>,
    pub encryption_key: Option<String>,

    // Session timing
    pub created_at: chrono::DateTime<Utc>,
    pub expires_at: chrono::DateTime<Utc>,
}

/// Client connection information
#[derive(Debug, Clone)]
pub struct ClientConnection {
    pub client_id: String,
    pub connected: bool,
    pub connected_at: Option<DateTime<Utc>>,
    pub last_seen: DateTime<Utc>,
    pub account_hash: Option<String>,
    pub device_hash: Option<String>,
}

/// Application state that is shared across all request handlers
#[derive(Clone)]
pub struct AppState {
    /// Server configuration
    pub config: Config,
    /// Storage service for persisting data
    pub storage: Arc<dyn Storage>,
    /// OAuth service for authentication
    pub oauth: OAuthService,
    /// Encryption service for handling keys
    pub encryption: EncryptionService,
    /// File service (file upload, download, synchronization)
    pub file: FileService,
    /// Device service (device registration, management)
    pub device: DeviceService,
    /// Version service for file version management
    pub version_service: VersionServiceImpl,
    /// Notification manager for broadcasting events
    pub notification_manager: Arc<NotificationManager>,
    /// Event bus for cross-instance broadcasting (noop by default)
    pub event_bus: Arc<dyn EventBus>,
    /// Shared authentication sessions between HTTP and gRPC
    pub auth_sessions: Arc<Mutex<HashMap<String, AuthSession>>>,
    /// Connection handler for managing connections
    pub connection_handler: Arc<RwLock<ConnectionHandler>>,
    /// Connection tracker for monitoring client states
    pub connection_tracker: Arc<ConnectionTracker>,
    /// Client store for tracking connections
    pub client_store: Arc<ClientStore>,
}

static APP_CONFIG: OnceCell<Config> = OnceCell::new();

impl AppState {
    async fn create_event_bus() -> Arc<dyn EventBus> {
        let cfg = crate::config::settings::MessageBrokerConfig::load();
        if cfg.enabled {
            match RabbitMqEventBus::connect(
                &cfg.url,
                &cfg.exchange,
                &cfg.queue_prefix,
                cfg.prefetch,
                cfg.durable,
            )
            .await
            {
                Ok(bus) => {
                    info!(
                        "RabbitMQ EventBus connected: exchange={} prefetch={}",
                        cfg.exchange, cfg.prefetch
                    );
                    Arc::new(bus)
                }
                Err(e) => {
                    error!("RabbitMQ connection failed, using NoopEventBus: {}", e);
                    Arc::new(NoopEventBus::default())
                }
            }
        } else {
            Arc::new(NoopEventBus::default())
        }
    }
    /// parse MySQL URL and initialize storage
    async fn initialize_storage(url: &str) -> Result<Arc<dyn Storage>, AppError> {
        if url.starts_with("mysql://") {
            info!("Using MySQL storage");
            info!("MySQL URL: {}", url);
            match MySqlStorage::new_with_url(url).await {
                Ok(storage) => {
                    // Log target database name via a simple query
                    match sqlx::query_scalar::<_, String>("SELECT DATABASE()")
                        .fetch_optional(storage.get_sqlx_pool())
                        .await
                    {
                        Ok(Some(db)) => info!("Connected to database: {}", db),
                        Ok(None) => info!("Connected to database: <unknown>"),
                        Err(e) => warn!("Failed to retrieve current database name: {}", e),
                    }
                    if let Err(e) = storage.init_schema().await {
                        error!("Failed to initialize MySQL schema: {}", e);
                        return Err(AppError::Storage(format!(
                            "Failed to initialize MySQL schema: {}",
                            e
                        )));
                    }
                    // run schema migrations (ALTERs) for existing deployments
                    if let Err(e) = storage.migrate_schema().await {
                        error!("Failed to migrate MySQL schema: {}", e);
                        return Err(AppError::Storage(format!(
                            "Failed to migrate MySQL schema: {}",
                            e
                        )));
                    }
                    Ok(Arc::new(storage))
                }
                Err(e) => {
                    error!("Failed to initialize MySQL storage: {}", e);
                    Err(AppError::Storage(format!(
                        "Failed to connect to MySQL: {}",
                        e
                    )))
                }
            }
        } else {
            info!("Unknown storage URL scheme, using in-memory storage");
            Ok(Arc::new(MemoryStorage::new()))
        }
    }

    /// initialize storage and other services
    async fn initialize_services(
        storage_url: Option<&String>,
    ) -> Result<
        (
            Arc<dyn Storage>,
            Arc<NotificationManager>,
            Arc<dyn EventBus>,
            OAuthService,
            EncryptionService,
            FileService,
            DeviceService,
            VersionServiceImpl,
        ),
        AppError,
    > {
        // initialize storage
        let storage = match storage_url {
            Some(url) => Self::initialize_storage(url).await?,
            None => {
                info!("No storage path specified, using in-memory storage");
                Arc::new(MemoryStorage::new())
            }
        };

        // initialize notification manager
        let notification_manager = Arc::new(NotificationManager::new_with_storage(storage.clone()));

        // initialize OAuth service
        let oauth = OAuthService::new(storage.clone());

        // initialize encryption service
        let encryption = EncryptionService::new(storage.clone());

        // initialize event bus (RabbitMQ if enabled)
        let event_bus: Arc<dyn EventBus> = Self::create_event_bus().await;

        // initialize file storage (load from settings)
        let file_storage = Self::initialize_file_storage().await?;

        // initialize file service (include notification manager and file storage)
        let file = FileService::with_storage_file_storage_and_notifications(
            storage.clone(),
            file_storage,
            notification_manager.clone(),
        );

        // initialize device service
        let device = DeviceService::with_storage(storage.clone());

        // initialize version service
        let version_service = VersionServiceImpl::new(storage.clone(), file.clone());

        Ok((
            storage,
            notification_manager,
            event_bus,
            oauth,
            encryption,
            file,
            device,
            version_service,
        ))
    }

    /// initialize file storage
    async fn initialize_file_storage() -> Result<Arc<dyn FileStorage>, AppError> {
        // load storage configuration from settings
        let storage_config = crate::config::settings::StorageConfig::load();

        info!(
            "Initializing file storage with type: {:?}",
            storage_config.storage_type
        );

        match crate::storage::create_file_storage(&storage_config).await {
            Ok(file_storage) => {
                info!(
                    "File storage initialized successfully: {}",
                    file_storage.storage_type()
                );
                Ok(file_storage)
            }
            Err(e) => {
                error!("Failed to initialize file storage: {}", e);
                Err(AppError::Storage(format!(
                    "Failed to initialize file storage: {}",
                    e
                )))
            }
        }
    }

    /// initialize file storage (use existing storage instance)
    async fn initialize_file_storage_with_storage(
        storage: Arc<dyn Storage>,
    ) -> Result<Arc<dyn FileStorage>, AppError> {
        // load storage configuration from settings
        let storage_config = crate::config::settings::StorageConfig::load();

        info!(
            "Initializing file storage with type: {:?}",
            storage_config.storage_type
        );

        // If using in-memory main storage, use in-memory file storage too
        if storage.as_any().downcast_ref::<MemoryStorage>().is_some() {
            struct SimpleInMemoryFileStorage;
            #[async_trait::async_trait]
            impl crate::storage::FileStorage for SimpleInMemoryFileStorage {
                async fn store_file_data(
                    &self,
                    _file_id: u64,
                    _data: Vec<u8>,
                ) -> crate::storage::Result<String> {
                    Ok("mem".to_string())
                }
                async fn store_file_data_with_options(
                    &self,
                    file_id: u64,
                    data: Vec<u8>,
                    _compress: bool,
                ) -> crate::storage::Result<String> {
                    self.store_file_data(file_id, data).await
                }
                async fn get_file_data(
                    &self,
                    _file_id: u64,
                ) -> crate::storage::Result<Option<Vec<u8>>> {
                    Ok(None)
                }
                async fn delete_file_data(&self, _file_id: u64) -> crate::storage::Result<()> {
                    Ok(())
                }
                async fn batch_delete_file_data(
                    &self,
                    file_ids: Vec<u64>,
                ) -> crate::storage::Result<Vec<(u64, bool)>> {
                    Ok(file_ids.into_iter().map(|id| (id, true)).collect())
                }
                async fn file_data_exists(&self, _file_id: u64) -> crate::storage::Result<bool> {
                    Ok(false)
                }
                async fn get_file_size(
                    &self,
                    _file_id: u64,
                ) -> crate::storage::Result<Option<u64>> {
                    Ok(None)
                }
                async fn health_check(&self) -> crate::storage::Result<()> {
                    Ok(())
                }
                async fn get_metrics(
                    &self,
                ) -> crate::storage::Result<crate::storage::StorageMetrics> {
                    Ok(crate::storage::StorageMetrics::default())
                }
                fn storage_type(&self) -> &'static str {
                    "memory"
                }
                async fn cleanup_orphaned_data(&self) -> crate::storage::Result<u64> {
                    Ok(0)
                }
            }
            return Ok(Arc::new(SimpleInMemoryFileStorage));
        }

        // MySQL Storage
        if storage_config.storage_type == crate::config::settings::StorageType::Database {
            // check if Storage trait object is MySqlStorage
            // already Arc<dyn Storage> so can't use directly
            // DatabaseFileStorage will create its own MySQL connection
            info!("Database storage type selected, DatabaseFileStorage will create its own MySQL connection");
        }

        // create default file storage
        match crate::storage::create_file_storage(&storage_config).await {
            Ok(file_storage) => {
                info!(
                    "File storage initialized successfully: {}",
                    file_storage.storage_type()
                );
                Ok(file_storage)
            }
            Err(e) => {
                error!("Failed to initialize file storage: {}", e);
                Err(AppError::Storage(format!(
                    "Failed to initialize file storage: {}",
                    e
                )))
            }
        }
    }

    /// initialize services using an existing storage (to avoid creating a separate in-memory storage)
    pub async fn new_with_storage_and_server_config(
        storage: Arc<dyn Storage>,
        config: &ServerConfig,
    ) -> Result<Self, AppError> {
        // Load full config via async loader to respect Secrets Manager and unified keys
        let mut full_config = crate::config::settings::Config::load_async()
            .await
            .unwrap_or_else(|_| crate::config::settings::Config::load());
        full_config.server = config.clone();

        // initialize notification manager
        let notification_manager = Arc::new(NotificationManager::new_with_storage(storage.clone()));

        // initialize OAuth service
        let oauth = OAuthService::new(storage.clone());

        // initialize encryption service
        let encryption = EncryptionService::new(storage.clone());

        // watcher service removed; handlers call storage directly

        // initialize file storage with existing storage context
        let file_storage = Self::initialize_file_storage_with_storage(storage.clone()).await?;

        // initialize file service (include notification manager and file storage)
        let file = FileService::with_storage_file_storage_and_notifications(
            storage.clone(),
            file_storage,
            notification_manager.clone(),
        );

        // initialize device service
        let device = DeviceService::with_storage(storage.clone());

        // initialize version service
        let version_service = VersionServiceImpl::new(storage.clone(), file.clone());

        // initialize event bus (RabbitMQ if enabled)
        let event_bus: Arc<dyn EventBus> = Self::create_event_bus().await;

        Ok(Self {
            config: full_config.clone(),
            storage,
            oauth,
            encryption,
            file,
            device,
            version_service,
            notification_manager: notification_manager.clone(),
            event_bus,
            auth_sessions: Arc::new(Mutex::new(HashMap::new())),
            connection_handler: Arc::new(RwLock::new(ConnectionHandler::new())),
            connection_tracker: Arc::new(ConnectionTracker::new()),
            client_store: Arc::new(ClientStore::new()),
        })
    }

    /// Create a new application state with given configuration
    pub async fn new(config: &Config) -> Result<Self, AppError> {
        let (
            storage,
            notification_manager,
            event_bus,
            oauth,
            encryption,
            file,
            device,
            version_service,
        ) = Self::initialize_services(config.server.storage_path.as_ref()).await?;
        let state = Self {
            config: config.clone(),
            storage,
            oauth,
            encryption,
            file,
            device,
            version_service,
            notification_manager,
            event_bus,
            auth_sessions: Arc::new(Mutex::new(HashMap::new())),
            connection_handler: Arc::new(RwLock::new(ConnectionHandler::new())),
            connection_tracker: Arc::new(ConnectionTracker::new()),
            client_store: Arc::new(ClientStore::new()),
        };
        let _ = APP_CONFIG.set(state.config.clone());
        Ok(state)
    }

    /// Create a new application state with given server configuration
    pub async fn new_from_server_config(config: &ServerConfig) -> Result<Self, AppError> {
        // create simple Config object
        let full_config = Config {
            server: config.clone(),
            database: crate::config::settings::DatabaseConfig::default(),
            logging: crate::config::settings::LoggingConfig::default(),
            features: crate::config::settings::FeatureFlags::default(),
            storage: crate::config::settings::StorageConfig::default(),
            message_broker: crate::config::settings::MessageBrokerConfig::load(),
            redis: crate::config::settings::RedisConfig::load(),
            server_encode_key: None,
        };

        let (
            storage,
            notification_manager,
            event_bus,
            oauth,
            encryption,
            file,
            device,
            version_service,
        ) = Self::initialize_services(config.storage_path.as_ref()).await?;

        // create AppState object
        let app_state = Self {
            config: full_config.clone(),
            storage,
            oauth,
            encryption,
            file,
            device,
            version_service,
            notification_manager: notification_manager.clone(),
            event_bus,
            auth_sessions: Arc::new(Mutex::new(HashMap::new())),
            connection_handler: Arc::new(RwLock::new(ConnectionHandler::new())),
            connection_tracker: Arc::new(ConnectionTracker::new()),
            client_store: Arc::new(ClientStore::new()),
        };
        notification_manager
            .set_transport_encrypt_metadata(full_config.features.transport_encrypt_metadata)
            .await;

        // Start background connection monitoring tasks
        let cleanup_scheduler =
            create_default_cleanup_scheduler(app_state.connection_tracker.clone());
        let stats_reporter = create_default_stats_reporter(app_state.connection_tracker.clone());

        // Start background tasks (fire and forget)
        tokio::spawn(async move {
            cleanup_scheduler.start().await;
        });

        tokio::spawn(async move {
            stats_reporter.start().await;
        });

        info!("🚀 Background connection monitoring tasks started");

        // later add statement to set app_state in connection_handler
        // currently skip to avoid circular reference

        let _ = APP_CONFIG.set(app_state.config.clone());
        Ok(app_state)
    }

    /// create watcher or return existing watcher ID
    pub async fn create_or_get_watcher(
        &self,
        account_hash: &str,
        group_id: i32,
        watcher_data: &crate::sync::WatcherData,
    ) -> Result<i32, crate::storage::StorageError> {
        info!(
            "Creating or getting watcher for account={}, group_id={}, folder={}",
            account_hash, group_id, &watcher_data.folder
        );

        // Validate watcher folder (numeric-only segment guard with whitelist/regex exceptions)
        if let Err(msg) = crate::utils::validator::validate_watcher_folder(&watcher_data.folder) {
            return Err(crate::storage::StorageError::ValidationError(msg));
        }

        // check if watcher already exists for this folder
        let existing_watcher = self
            .storage
            .find_watcher_by_folder(account_hash, group_id, &watcher_data.folder)
            .await?;

        if let Some(watcher_id) = existing_watcher {
            info!(
                "Found existing watcher with ID: {} for folder: {}",
                watcher_id, &watcher_data.folder
            );
            // Upsert conditions for existing watcher (replace all)
            use crate::models::watcher::{ConditionType, WatcherCondition};
            let now = chrono::Utc::now();

            // Guard: if client sends both arrays empty, preserve server-side existing conditions
            let incoming_empty = watcher_data.union_conditions.is_empty()
                && watcher_data.subtracting_conditions.is_empty();
            if !incoming_empty {
                let mut conditions: Vec<WatcherCondition> = Vec::new();
                for cd in &watcher_data.union_conditions {
                    conditions.push(WatcherCondition {
                        id: None,
                        account_hash: account_hash.to_string(),
                        watcher_id,
                        local_watcher_id: watcher_data.watcher_id,
                        local_group_id: group_id,
                        condition_type: ConditionType::Union,
                        key: cd.key.clone(),
                        value: cd.value.clone(),
                        operator: "equals".to_string(),
                        created_at: now,
                        updated_at: now,
                    });
                }
                for cd in &watcher_data.subtracting_conditions {
                    conditions.push(WatcherCondition {
                        id: None,
                        account_hash: account_hash.to_string(),
                        watcher_id,
                        local_watcher_id: watcher_data.watcher_id,
                        local_group_id: group_id,
                        condition_type: ConditionType::Subtract,
                        key: cd.key.clone(),
                        value: cd.value.clone(),
                        operator: "equals".to_string(),
                        created_at: now,
                        updated_at: now,
                    });
                }
                if let Err(e) = self
                    .storage
                    .save_watcher_conditions(watcher_id, &conditions)
                    .await
                {
                    error!(
                        "Failed to save watcher conditions for watcher {}: {}",
                        watcher_id, e
                    );
                    return Err(e);
                }
            } else {
                debug!("Incoming conditions are empty; preserving existing watcher conditions: watcher_id={}", watcher_id);
            }
            // return existing watcher ID
            return Ok(watcher_id);
        }

        info!(
            "No existing watcher found for folder: {}, creating new watcher",
            &watcher_data.folder
        );
        // create new watcher
        let now = chrono::Utc::now().timestamp();

        // create new watcher ID (include conditions)
        let new_watcher_id = match self
            .storage
            .create_watcher_with_conditions(account_hash, group_id, watcher_data, now)
            .await
        {
            Ok(id) => {
                info!(
                    "Successfully created new watcher with ID: {} for folder: {}",
                    id, &watcher_data.folder
                );
                id
            }
            Err(e) => {
                error!(
                    "Failed to create watcher for folder {}: {}",
                    &watcher_data.folder, e
                );
                return Err(e);
            }
        };

        Ok(new_watcher_id)
    }

    /// broadcast WatcherGroup update
    pub async fn broadcast_watcher_group_update(
        &self,
        account_hash: &str,
        device_hash: &str,
        group_id: i32,
        update_type: crate::sync::watcher_group_update_notification::UpdateType,
    ) -> Result<(), crate::storage::StorageError> {
        use crate::sync::{
            watcher_group_update_notification::UpdateType, WatcherGroupUpdateNotification,
        };

        // get group data
        let group_data = self
            .storage
            .get_watcher_group_by_account_and_id(account_hash, group_id)
            .await?;

        // create notification to broadcast
        let notification = WatcherGroupUpdateNotification {
            account_hash: account_hash.to_string(),
            device_hash: device_hash.to_string(),
            group_data,
            update_type: update_type as i32,
            timestamp: chrono::Utc::now().timestamp(),
        };

        // broadcast to other devices (exclude the device that sent the update)
        if let Err(e) = self
            .notification_manager
            .broadcast_watcher_group_update(account_hash, Some(device_hash), notification)
            .await
        {
            error!("Failed to broadcast WatcherGroup update: {}", e);
        }
        // Publish to cross-instance bus (noop by default)
        let routing_key = format!("watcher.group.update.{}", account_hash);
        let payload = serde_json::json!({
            "type": "watcher_group_update",
            "id": nanoid::nanoid!(8),
            "account_hash": account_hash,
            "device_hash": device_hash,
            "timestamp": chrono::Utc::now().timestamp(),
        })
        .to_string()
        .into_bytes();
        if let Err(e) = self.event_bus.publish(&routing_key, payload).await {
            debug!("EventBus publish failed (noop or disconnected): {}", e);
        }

        Ok(())
    }

    /// broadcast WatcherPreset update
    pub async fn broadcast_watcher_preset_update(
        &self,
        account_hash: &str,
        device_hash: &str,
        presets: Vec<String>,
        update_type: crate::sync::watcher_preset_update_notification::UpdateType,
    ) -> Result<(), crate::storage::StorageError> {
        use crate::sync::{
            watcher_preset_update_notification::UpdateType, WatcherPresetUpdateNotification,
        };

        // create notification to broadcast
        let notification = WatcherPresetUpdateNotification {
            account_hash: account_hash.to_string(),
            device_hash: device_hash.to_string(),
            presets,
            update_type: update_type as i32,
            timestamp: chrono::Utc::now().timestamp(),
        };

        // broadcast to other devices (exclude the device that sent the update)
        if let Err(e) = self
            .notification_manager
            .broadcast_watcher_preset_update(account_hash, Some(device_hash), notification)
            .await
        {
            error!("Failed to broadcast WatcherPreset update: {}", e);
        }
        // Publish to cross-instance bus (noop by default)
        let routing_key = format!("watcher.preset.update.{}", account_hash);
        let payload = serde_json::json!({
            "type": "watcher_preset_update",
            "id": nanoid::nanoid!(8),
            "account_hash": account_hash,
            "device_hash": device_hash,
            "timestamp": chrono::Utc::now().timestamp(),
        })
        .to_string()
        .into_bytes();
        if let Err(e) = self.event_bus.publish(&routing_key, payload).await {
            debug!("EventBus publish failed (noop or disconnected): {}", e);
        }

        Ok(())
    }

    /// Get the client store
    pub fn get_client_store(&self) -> Option<ClientStore> {
        // Return the actual client store instance
        Some((*self.client_store).clone())
    }

    /// Start background cleanup task for client store
    pub fn start_client_cleanup_task(&self) {
        let client_store = self.client_store.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600)); // 1 hour

            loop {
                interval.tick().await;

                // Clean up disconnected clients older than 24 hours
                client_store.cleanup_disconnected_clients(24).await;

                // Log connection statistics
                let stats = client_store.get_connection_stats().await;
                info!("Client connection stats: {:?}", stats);
            }
        });
    }

    /// Start background cleanup task for file retention (TTL and revision trimming)
    pub fn start_retention_cleanup_task(&self) {
        let storage = self.storage.clone();
        let file_ttl_secs = self.config.storage.file_ttl_secs;
        let max_revisions = self.config.storage.max_file_revisions;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600)); // hourly
            loop {
                interval.tick().await;
                debug!(
                    "Running retention cleanup: ttl_secs={}, max_revisions={}",
                    file_ttl_secs, max_revisions
                );
                if let Err(e) = storage.trim_old_revisions(max_revisions).await {
                    warn!("trim_old_revisions failed: {}", e);
                }
                if let Err(e) = storage.purge_deleted_files_older_than(file_ttl_secs).await {
                    warn!("purge_deleted_files_older_than failed: {}", e);
                }
            }
        });
    }

    pub fn get_config() -> Config {
        APP_CONFIG.get().cloned().unwrap_or_else(|| Config::load())
    }
}

/// Client store for tracking connected clients
#[derive(Clone)]
pub struct ClientStore {
    /// Connected clients mapping (client_id -> connection info)
    connections: Arc<DashMap<String, ClientConnection>>,
}

impl ClientStore {
    /// Create new ClientStore instance
    pub fn new() -> Self {
        Self {
            connections: Arc::new(DashMap::new()),
        }
    }

    /// Set client connection status
    pub async fn set_client_connected(&self, client_id: &str, connected: bool) {
        info!(
            "Setting client {} connection status to: {}",
            client_id, connected
        );

        let now = Utc::now();

        self.connections
            .entry(client_id.to_string())
            .and_modify(|conn| {
                conn.connected = connected;
                conn.last_seen = now;
                if connected {
                    conn.connected_at = Some(now);
                }
            })
            .or_insert_with(|| ClientConnection {
                client_id: client_id.to_string(),
                connected,
                connected_at: if connected { Some(now) } else { None },
                last_seen: now,
                account_hash: None,
                device_hash: None,
            });

        if connected {
            info!("✅ Client {} connected at {}", client_id, now);
        } else {
            info!("❌ Client {} disconnected at {}", client_id, now);
        }
    }

    /// Update client authentication information
    pub async fn update_client_auth(
        &self,
        client_id: &str,
        account_hash: Option<String>,
        device_hash: Option<String>,
    ) {
        self.connections
            .entry(client_id.to_string())
            .and_modify(|conn| {
                conn.account_hash = account_hash.clone();
                conn.device_hash = device_hash.clone();
                conn.last_seen = Utc::now();
            });

        debug!(
            "Updated auth info for client {}: account_hash={:?}, device_hash={:?}",
            client_id, account_hash, device_hash
        );
    }

    /// Check if client is connected
    pub async fn is_client_connected(&self, client_id: &str) -> bool {
        self.connections
            .get(client_id)
            .map(|conn| conn.connected)
            .unwrap_or(false)
    }

    /// Get client connection info
    pub async fn get_client_connection(&self, client_id: &str) -> Option<ClientConnection> {
        self.connections.get(client_id).map(|conn| conn.clone())
    }

    /// Get all connected clients
    pub async fn get_connected_clients(&self) -> Vec<ClientConnection> {
        self.connections
            .iter()
            .filter(|entry| entry.value().connected)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get connected clients by account
    pub async fn get_connected_clients_by_account(
        &self,
        account_hash: &str,
    ) -> Vec<ClientConnection> {
        self.connections
            .iter()
            .filter(|entry| {
                let conn = entry.value();
                conn.connected
                    && conn
                        .account_hash
                        .as_ref()
                        .map(|h| h == account_hash)
                        .unwrap_or(false)
            })
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Remove disconnected clients (cleanup)
    pub async fn cleanup_disconnected_clients(&self, retention_hours: i64) {
        let cutoff_time = Utc::now() - chrono::Duration::hours(retention_hours);

        let to_remove: Vec<String> = self
            .connections
            .iter()
            .filter(|entry| {
                let conn = entry.value();
                !conn.connected && conn.last_seen < cutoff_time
            })
            .map(|entry| entry.key().clone())
            .collect();

        for client_id in to_remove {
            self.connections.remove(&client_id);
            debug!("Cleaned up disconnected client: {}", client_id);
        }
    }

    /// Get connection statistics
    pub async fn get_connection_stats(&self) -> HashMap<String, usize> {
        let mut stats = HashMap::new();
        let mut connected_count = 0;
        let mut total_count = 0;

        for entry in self.connections.iter() {
            total_count += 1;
            if entry.value().connected {
                connected_count += 1;
            }
        }

        stats.insert("total".to_string(), total_count);
        stats.insert("connected".to_string(), connected_count);
        stats.insert("disconnected".to_string(), total_count - connected_count);

        stats
    }
}
