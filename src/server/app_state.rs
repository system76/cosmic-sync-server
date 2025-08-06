use std::sync::Arc;
use std::collections::HashMap;
use tracing::{info, warn, error, debug};
use crate::storage::{Storage, memory::MemoryStorage, FileStorage};
    use crate::storage::mysql::MySqlStorage;
use crate::storage::mysql_watcher::MySqlWatcherExt;
use crate::services::encryption_service::EncryptionService;
use crate::auth::oauth::OAuthService;
use crate::config::settings::{Config, ServerConfig};
use crate::services::watcher_service::WatcherService;
use crate::services::file_service::FileService;
use crate::services::device_service::DeviceService;
use crate::services::version_service::{VersionService, VersionServiceImpl};
use crate::error::AppError;
use crate::server::notification_manager::NotificationManager;
use std::sync::Mutex;
use chrono::{Utc, Duration, DateTime};
use tokio::sync::RwLock;
use std::path::PathBuf;
use std::fs;
use std::fs::File;
use std::io::Write;
use crate::server::connection_handler::ConnectionHandler;
use crate::server::connection_tracker::ConnectionTracker;
use crate::server::connection_cleanup::{create_default_cleanup_scheduler, create_default_stats_reporter};
use dashmap::DashMap;
use mysql_async::Opts;

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
    /// Watcher service (directory monitoring, event handling)
    pub watcher: WatcherService,
    /// File service (file upload, download, synchronization)
    pub file: FileService,
    /// Device service (device registration, management)
    pub device: DeviceService,
    /// Version service for file version management
    pub version_service: VersionServiceImpl,
    /// Notification manager for broadcasting events
    pub notification_manager: Arc<NotificationManager>,
    /// Shared authentication sessions between HTTP and gRPC
    pub auth_sessions: Arc<Mutex<HashMap<String, AuthSession>>>,
    /// Connection handler for managing connections
    pub connection_handler: Arc<RwLock<ConnectionHandler>>,
    /// Connection tracker for monitoring client states
    pub connection_tracker: Arc<ConnectionTracker>,
    /// Client store for tracking connections
    pub client_store: Arc<ClientStore>,
}

impl AppState {
    /// parse MySQL URL and initialize storage
    async fn initialize_storage(url: &str) -> Result<Arc<dyn Storage>, AppError> {
        if url.starts_with("mysql://") {
            info!("Using MySQL storage");
            // parse MySQL connection URL (mysql://user:password@host:port/database format)
            let mut parts = url.split("://");
            let _ = parts.next(); // skip "mysql" part
            
            if let Some(conn_info) = parts.next() {
                let mut auth_host_parts = conn_info.split('@');
                
                // extract user info
                let auth_part = auth_host_parts.next().unwrap_or("");
                let mut auth_parts = auth_part.split(':');
                let user = auth_parts.next().unwrap_or("");
                let password = auth_parts.next().unwrap_or("");
                
                // extract host info
                let host_part = auth_host_parts.next().unwrap_or("");
                let mut host_parts = host_part.split('/');
                let host_port = host_parts.next().unwrap_or("");
                let mut host_port_parts = host_port.split(':');
                let host = host_port_parts.next().unwrap_or("");
                let port_str = host_port_parts.next().unwrap_or("3306");
                let port = port_str.parse::<u16>().unwrap_or(3306);
                
                // extract database name
                let database = host_parts.next().unwrap_or("");
                
                // 연결 URL 생성
                let connection_url = format!("mysql://{}:{}@{}:{}/{}", user, password, host, port, database);
                
                // Opts 생성
                let opts = mysql_async::Opts::from_url(&connection_url)
                    .map_err(|e| AppError::Storage(format!("Failed to parse MySQL connection URL: {}", e)))?;
                
                // initialize MySQL storage
                match MySqlStorage::new(opts) {
                    Ok(storage) => {
                        // initialize schema
                        if let Err(e) = storage.init_schema().await {
                            error!("Failed to initialize MySQL schema: {}", e);
                            return Err(AppError::Storage(format!("Failed to initialize MySQL schema: {}", e)));
                        }
                        Ok(Arc::new(storage))
                    },
                    Err(e) => {
                        error!("Failed to initialize MySQL storage: {}", e);
                        Err(AppError::Storage(format!("Failed to connect to MySQL: {}", e)))
                    }
                }
            } else {
                Err(AppError::Config("Invalid MySQL URL format".to_string()))
            }
        } else {
            info!("Unknown storage URL scheme, using in-memory storage");
            Ok(Arc::new(MemoryStorage::new()))
        }
    }
    
    /// initialize storage and other services
    async fn initialize_services(storage_url: Option<&String>) -> Result<(Arc<dyn Storage>, Arc<NotificationManager>, OAuthService, EncryptionService, WatcherService, FileService, DeviceService, VersionServiceImpl), AppError> {
        // initialize storage
        let storage = match storage_url {
            Some(url) => Self::initialize_storage(url).await?,
            None => {
                info!("No storage path specified, using in-memory storage");
                Arc::new(MemoryStorage::new())
            }
        };
        
        // initialize notification manager
        let notification_manager = Arc::new(NotificationManager::new());
        
        // initialize OAuth service
        let oauth = OAuthService::new(storage.clone());
        
        // initialize encryption service
        let encryption = EncryptionService::new(storage.clone());
        
        // initialize watcher service
        let watcher = WatcherService::with_storage(storage.clone());
        
        // initialize file storage (load from settings)
        let file_storage = Self::initialize_file_storage().await?;
        
        // initialize file service (include notification manager and file storage)
        let file = FileService::with_storage_file_storage_and_notifications(
            storage.clone(), 
            file_storage,
            notification_manager.clone()
        );
        
        // initialize device service
        let device = DeviceService::with_storage(storage.clone());
        
        // initialize version service
        let version_service = VersionServiceImpl::new(storage.clone(), file.clone());
        
        Ok((storage, notification_manager, oauth, encryption, watcher, file, device, version_service))
    }
    
    /// initialize file storage
    async fn initialize_file_storage() -> Result<Arc<dyn FileStorage>, AppError> {
        // load storage configuration from settings
        let storage_config = crate::config::settings::StorageConfig::load();
        
        info!("Initializing file storage with type: {:?}", storage_config.storage_type);
        
        match crate::storage::create_file_storage(&storage_config).await {
            Ok(file_storage) => {
                info!("File storage initialized successfully: {}", file_storage.storage_type());
                Ok(file_storage)
            }
            Err(e) => {
                error!("Failed to initialize file storage: {}", e);
                Err(AppError::Storage(format!("Failed to initialize file storage: {}", e)))
            }
        }
    }
    
    /// initialize file storage (use existing storage instance)
    async fn initialize_file_storage_with_storage(storage: Arc<dyn Storage>) -> Result<Arc<dyn FileStorage>, AppError> {
        // load storage configuration from settings
        let storage_config = crate::config::settings::StorageConfig::load();
        
        info!("Initializing file storage with type: {:?}", storage_config.storage_type);
        
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
                info!("File storage initialized successfully: {}", file_storage.storage_type());
                Ok(file_storage)
            }
            Err(e) => {
                error!("Failed to initialize file storage: {}", e);
                Err(AppError::Storage(format!("Failed to initialize file storage: {}", e)))
            }
        }
    }

    /// Create a new application state with given configuration
    pub async fn new(config: &Config) -> Result<Self, AppError> {
        let (storage, notification_manager, oauth, encryption, watcher, file, device, version_service) = 
            Self::initialize_services(config.server.storage_path.as_ref()).await?;
        
        Ok(Self {
            config: config.clone(),
            storage,
            oauth,
            encryption,
            watcher,
            file,
            device,
            version_service,
            notification_manager,
            auth_sessions: Arc::new(Mutex::new(HashMap::new())),
            connection_handler: Arc::new(RwLock::new(ConnectionHandler::new())),
            connection_tracker: Arc::new(ConnectionTracker::new()),
            client_store: Arc::new(ClientStore::new()),
        })
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
        };
        
        let (storage, notification_manager, oauth, encryption, watcher, file, device, version_service) = 
            Self::initialize_services(config.storage_path.as_ref()).await?;
        
        // create AppState object
        let app_state = Self {
            config: full_config,
            storage,
            oauth,
            encryption,
            watcher,
            file,
            device,
            version_service,
            notification_manager,
            auth_sessions: Arc::new(Mutex::new(HashMap::new())),
            connection_handler: Arc::new(RwLock::new(ConnectionHandler::new())),
            connection_tracker: Arc::new(ConnectionTracker::new()),
            client_store: Arc::new(ClientStore::new()),
        };
        
        // Start background connection monitoring tasks
        let cleanup_scheduler = create_default_cleanup_scheduler(app_state.connection_tracker.clone());
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
        
        Ok(app_state)
    }

    /// create watcher or return existing watcher ID
    pub async fn create_or_get_watcher(
        &self,
        account_hash: &str,
        group_id: i32,
        watcher_data: &crate::sync::WatcherData,
    ) -> Result<i32, crate::storage::StorageError> {
        info!("Creating or getting watcher for account={}, group_id={}, folder={}", 
              account_hash, group_id, &watcher_data.folder);
        
        // check if watcher already exists for this folder
        let existing_watcher = self.storage.find_watcher_by_folder(account_hash, group_id, &watcher_data.folder).await?;
        
        if let Some(watcher_id) = existing_watcher {
            info!("Found existing watcher with ID: {} for folder: {}", watcher_id, &watcher_data.folder);
            // return existing watcher ID
            return Ok(watcher_id);
        }
        
        info!("No existing watcher found for folder: {}, creating new watcher", &watcher_data.folder);
        // create new watcher
        let now = chrono::Utc::now().timestamp();
        
        // create new watcher ID (include conditions)
        let new_watcher_id = match self.storage.create_watcher_with_conditions(
            account_hash,
            group_id,
            watcher_data,
            now,
        ).await {
            Ok(id) => {
                info!("Successfully created new watcher with ID: {} for folder: {}", id, &watcher_data.folder);
                id
            },
            Err(e) => {
                error!("Failed to create watcher for folder {}: {}", &watcher_data.folder, e);
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
        use crate::sync::{WatcherGroupUpdateNotification, watcher_group_update_notification::UpdateType};
        
        // get group data
        let group_data = self.storage.get_watcher_group_by_account_and_id(account_hash, group_id).await?;
        
        // create notification to broadcast
        let notification = WatcherGroupUpdateNotification {
            account_hash: account_hash.to_string(),
            device_hash: device_hash.to_string(),
            group_data,
            update_type: update_type as i32,
            timestamp: chrono::Utc::now().timestamp(),
        };
        
        // broadcast to other devices (exclude the device that sent the update)
        if let Err(e) = self.notification_manager.broadcast_watcher_group_update(
            account_hash,
            Some(device_hash),
            notification
        ).await {
            error!("Failed to broadcast WatcherGroup update: {}", e);
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
        use crate::sync::{WatcherPresetUpdateNotification, watcher_preset_update_notification::UpdateType};
        
        // create notification to broadcast
        let notification = WatcherPresetUpdateNotification {
                account_hash: account_hash.to_string(),
                device_hash: device_hash.to_string(),
            presets,
                update_type: update_type as i32,
                timestamp: chrono::Utc::now().timestamp(),
            };
            
        // broadcast to other devices (exclude the device that sent the update)
        if let Err(e) = self.notification_manager.broadcast_watcher_preset_update(
            account_hash,
            Some(device_hash),
            notification
        ).await {
            error!("Failed to broadcast WatcherPreset update: {}", e);
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
        info!("Setting client {} connection status to: {}", client_id, connected);
        
        let now = Utc::now();
        
        self.connections.entry(client_id.to_string()).and_modify(|conn| {
            conn.connected = connected;
            conn.last_seen = now;
            if connected {
                conn.connected_at = Some(now);
            }
        }).or_insert_with(|| {
            ClientConnection {
                client_id: client_id.to_string(),
                connected,
                connected_at: if connected { Some(now) } else { None },
                last_seen: now,
                account_hash: None,
                device_hash: None,
            }
        });
        
        if connected {
            info!("✅ Client {} connected at {}", client_id, now);
        } else {
            info!("❌ Client {} disconnected at {}", client_id, now);
        }
    }
    
    /// Update client authentication information
    pub async fn update_client_auth(&self, client_id: &str, account_hash: Option<String>, device_hash: Option<String>) {
        self.connections.entry(client_id.to_string()).and_modify(|conn| {
            conn.account_hash = account_hash.clone();
            conn.device_hash = device_hash.clone();
            conn.last_seen = Utc::now();
        });
        
        debug!("Updated auth info for client {}: account_hash={:?}, device_hash={:?}", 
               client_id, account_hash, device_hash);
    }
    
    /// Check if client is connected
    pub async fn is_client_connected(&self, client_id: &str) -> bool {
        self.connections.get(client_id)
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
    pub async fn get_connected_clients_by_account(&self, account_hash: &str) -> Vec<ClientConnection> {
        self.connections
            .iter()
            .filter(|entry| {
                let conn = entry.value();
                conn.connected && 
                conn.account_hash.as_ref().map(|h| h == account_hash).unwrap_or(false)
            })
            .map(|entry| entry.value().clone())
            .collect()
    }
    
    /// Remove disconnected clients (cleanup)
    pub async fn cleanup_disconnected_clients(&self, retention_hours: i64) {
        let cutoff_time = Utc::now() - chrono::Duration::hours(retention_hours);
        
        let to_remove: Vec<String> = self.connections
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