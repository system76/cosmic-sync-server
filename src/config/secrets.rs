use std::collections::HashMap;
use std::env;
use aws_config::BehaviorVersion;
use aws_sdk_secretsmanager::Client as SecretsClient;
use aws_sdk_secretsmanager::Error as SecretsError;
use serde_json::Value;
use tracing::{debug, error, info, warn};

/// Environment type for configuration loading
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Environment {
    Development,
    Staging,
    Production,
}

impl Environment {
    /// Get current environment from ENVIRONMENT environment variable (infrastructure standard)
    pub fn current() -> Self {
        match env::var("ENVIRONMENT")
            .unwrap_or_else(|_| "development".to_string())
            .to_lowercase().as_str() 
        {
            "staging" => Environment::Staging,
            "production" => Environment::Production,
            _ => Environment::Development,
        }
    }

    /// Check if this is a cloud environment (staging or production)
    pub fn is_cloud(&self) -> bool {
        matches!(self, Environment::Staging | Environment::Production)
    }
}

/// Configuration loader that handles both local environment variables and AWS Secrets Manager
pub struct ConfigLoader {
    environment: Environment,
    secrets_client: Option<SecretsClient>,
}

impl ConfigLoader {
    /// Create a new configuration loader
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let environment = Environment::current();
        info!("Initializing config loader for environment: {:?}", environment);

        let secrets_client = if environment.is_cloud() {
            info!("Cloud environment detected, initializing AWS Secrets Manager client");
            match Self::create_secrets_client().await {
                Ok(client) => Some(client),
                Err(e) => {
                    error!("Failed to create AWS Secrets Manager client: {}", e);
                    return Err(e);
                }
            }
        } else {
            info!("Development environment detected, using local environment variables");
            None
        };

        Ok(Self {
            environment,
            secrets_client,
        })
    }

    /// Create AWS Secrets Manager client
    async fn create_secrets_client() -> Result<SecretsClient, Box<dyn std::error::Error>> {
        let config = aws_config::defaults(BehaviorVersion::latest())
            .load()
            .await;
        
        Ok(SecretsClient::new(&config))
    }

    /// Get AWS secret name based on environment (infrastructure standard pattern)
    fn get_secret_name(&self) -> Option<String> {
        // First check if custom secret name is provided via environment variable
        if let Ok(custom_name) = env::var("AWS_SECRET_NAME") {
            info!("Using custom AWS secret name: {}", custom_name);
            return Some(custom_name);
        }

        // Generate secret name based on environment using infrastructure patterns
        let secret_name = match self.environment {
            Environment::Staging => "staging/so-dod/cosmic-sync/config",
            Environment::Production => "production/pop-os/cosmic-sync/config",
            Environment::Development => {
                warn!("Development environment should not use AWS Secrets Manager");
                return None;
            }
        };

        info!("Using infrastructure standard secret name: {}", secret_name);
        Some(secret_name.to_string())
    }

    /// Parse file size from configuration with support for string formats like "2MB", "50MB"
    async fn parse_file_size(&self, key: &str, default: &str) -> usize {
        let size_str = self.get_config_value(key, Some(default)).await
            .unwrap_or_else(|| default.to_string());
        
        // Handle string formats like "2MB", "50MB", "1GB"
        let size_str_lower = size_str.to_lowercase();
        if size_str_lower.ends_with("gb") {
            let num_str = size_str_lower.trim_end_matches("gb");
            if let Ok(gb) = num_str.parse::<usize>() {
                return gb * 1024 * 1024 * 1024;
            }
        } else if size_str_lower.ends_with("mb") {
            let num_str = size_str_lower.trim_end_matches("mb");
            if let Ok(mb) = num_str.parse::<usize>() {
                return mb * 1024 * 1024;
            }
        } else if size_str_lower.ends_with("kb") {
            let num_str = size_str_lower.trim_end_matches("kb");
            if let Ok(kb) = num_str.parse::<usize>() {
                return kb * 1024;
            }
        }
        
        // Handle pure numbers (assumed to be bytes)
        size_str.parse().unwrap_or(50 * 1024 * 1024) // Default to 50MB
    }

    /// Get configuration value, preferring Secrets Manager in cloud environments
    pub async fn get_config_value(&self, key: &str, default: Option<&str>) -> Option<String> {
        debug!("Getting config value for key: {}", key);

        // First try environment variable (always available as fallback)
        if let Ok(value) = env::var(key) {
            debug!("Found value for {} in environment variables", key);
            return Some(value);
        }

        // In cloud environments, try Secrets Manager
        if self.environment.is_cloud() {
            if let Some(value) = self.get_from_secrets_manager(key).await {
                debug!("Found value for {} in AWS Secrets Manager", key);
                return Some(value);
            }
        }

        // Return default if provided
        if let Some(default_value) = default {
            debug!("Using default value for {}", key);
            Some(default_value.to_string())
        } else {
            warn!("No value found for config key: {}", key);
            None
        }
    }

    /// Get value from AWS Secrets Manager
    async fn get_from_secrets_manager(&self, key: &str) -> Option<String> {
        let client = self.secrets_client.as_ref()?;
        
        // Get the secret name based on environment
        let secret_name = self.get_secret_name()?;
        
        debug!("Fetching secret '{}' from AWS Secrets Manager", secret_name);

        match client.get_secret_value()
            .secret_id(&secret_name)
            .send()
            .await
        {
            Ok(result) => {
                if let Some(secret_string) = result.secret_string() {
                    match serde_json::from_str::<HashMap<String, Value>>(secret_string) {
                        Ok(secrets_map) => {
                            if let Some(value) = secrets_map.get(key) {
                                match value {
                                    Value::String(s) => Some(s.clone()),
                                    Value::Number(n) => Some(n.to_string()),
                                    Value::Bool(b) => Some(b.to_string()),
                                    _ => {
                                        warn!("Unsupported value type for key {} in secrets", key);
                                        None
                                    }
                                }
                            } else {
                                debug!("Key {} not found in secrets", key);
                                None
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse secrets JSON: {}", e);
                            None
                        }
                    }
                } else {
                    warn!("Secret string is empty for secret: {}", secret_name);
                    None
                }
            }
            Err(e) => {
                error!("Failed to retrieve secret from AWS Secrets Manager: {}", e);
                None
            }
        }
    }

    /// Get database configuration from secrets or environment
    pub async fn get_database_config(&self) -> super::settings::DatabaseConfig {
        use super::settings::DatabaseConfig;

        let user = self.get_config_value("DB_USER", Some("user")).await
            .unwrap_or_else(|| "user".to_string());
        let password = self.get_config_value("DB_PASS", Some("password")).await
            .unwrap_or_else(|| "password".to_string());
        let name = self.get_config_value("DB_NAME", Some("cosmic_sync")).await
            .unwrap_or_else(|| "cosmic_sync".to_string());
        let host = self.get_config_value("DB_HOST", Some("localhost")).await
            .unwrap_or_else(|| "localhost".to_string());
        
        let port = self.get_config_value("DB_PORT", Some("3306")).await
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(3306);
        let max_connections = self.get_config_value("DB_POOL", Some("5")).await
            .and_then(|c| c.parse::<u32>().ok())
            .unwrap_or(5);
        let connection_timeout = self.get_config_value("DATABASE_CONNECTION_TIMEOUT", Some("30")).await
            .and_then(|t| t.parse::<u64>().ok())
            .unwrap_or(30);
        let log_queries = self.get_config_value("DATABASE_LOG_QUERIES", Some("false")).await
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        DatabaseConfig {
            user,
            password,
            name,
            host,
            port,
            max_connections,
            connection_timeout,
            log_queries,
        }
    }

    /// Get S3 configuration from secrets or environment (infrastructure standard)
    pub async fn get_s3_config(&self) -> super::settings::S3Config {
        use super::settings::S3Config;

        // Use infrastructure standard environment variable names
        let region = self.get_config_value("AWS_S3_REGION", Some("us-west-2")).await
            .unwrap_or_else(|| "us-west-2".to_string());
        let bucket = self.get_config_value("AWS_S3_BUCKET", Some("cosmic-sync-files")).await
            .unwrap_or_else(|| "cosmic-sync-files".to_string());
        let key_prefix = self.get_config_value("S3_KEY_PREFIX", Some("files/")).await
            .unwrap_or_else(|| "files/".to_string());
        
        let access_key_id = self.get_config_value("AWS_ACCESS_KEY_ID", None).await;
        let secret_access_key = self.get_config_value("AWS_SECRET_ACCESS_KEY", None).await;
        let session_token = self.get_config_value("AWS_SESSION_TOKEN", None).await;
        let endpoint_url = self.get_config_value("S3_ENDPOINT_URL", None).await;
        
        let force_path_style = self.get_config_value("S3_FORCE_PATH_STYLE", Some("false")).await
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        
        let use_secret_manager = self.environment.is_cloud();
        let secret_name = self.get_secret_name();
        
        let timeout_seconds = self.get_config_value("S3_TIMEOUT_SECONDS", Some("30")).await
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        let max_retries = self.get_config_value("S3_MAX_RETRIES", Some("3")).await
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);

        S3Config {
            region,
            bucket,
            key_prefix,
            access_key_id,
            secret_access_key,
            session_token,
            endpoint_url,
            force_path_style,
            use_secret_manager,
            secret_name,
            timeout_seconds,
            max_retries,
        }
    }

    /// Get server configuration from secrets or environment
    pub async fn get_server_config(&self) -> super::settings::ServerConfig {
        use super::settings::ServerConfig;

        let host = self.get_config_value("SERVER_HOST", Some(crate::config::constants::DEFAULT_GRPC_HOST)).await
            .unwrap_or_else(|| crate::config::constants::DEFAULT_GRPC_HOST.to_string());
        
        // Use GRPC_PORT (infrastructure standard)
        let port = self.get_config_value("GRPC_PORT", Some(&crate::config::constants::DEFAULT_GRPC_PORT.to_string())).await
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_GRPC_PORT);
        
        let storage_path = self.get_config_value("STORAGE_PATH", None).await;
        let worker_threads = self.get_config_value("WORKER_THREADS", Some(&crate::config::constants::DEFAULT_WORKER_THREADS.to_string())).await
            .and_then(|t| t.parse::<usize>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_WORKER_THREADS);
        let auth_token_expiry_hours = self.get_config_value("AUTH_TOKEN_EXPIRY_HOURS", Some(&crate::config::constants::DEFAULT_AUTH_TOKEN_EXPIRY_HOURS.to_string())).await
            .and_then(|h| h.parse::<i64>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_AUTH_TOKEN_EXPIRY_HOURS);
        
        // Parse file size with support for string formats like "2MB", "50MB"
        let max_file_size = self.parse_file_size("MAX_FILE_SIZE", &crate::config::constants::DEFAULT_MAX_FILE_SIZE_BYTES.to_string()).await;
        
        let max_concurrent_requests = self.get_config_value("MAX_CONCURRENT_REQUESTS", Some(&crate::config::constants::DEFAULT_MAX_CONCURRENT_REQUESTS.to_string())).await
            .and_then(|r| r.parse::<usize>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_MAX_CONCURRENT_REQUESTS);

        ServerConfig {
            host,
            port,
            storage_path,
            worker_threads,
            auth_token_expiry_hours,
            max_file_size,
            max_concurrent_requests,
        }
    }

    /// Get storage configuration from secrets or environment
    pub async fn get_storage_config(&self) -> super::settings::StorageConfig {
        use super::settings::{StorageConfig, StorageType};

        let storage_type = self.get_config_value("STORAGE_TYPE", Some("database")).await
            .unwrap_or_else(|| "database".to_string())
            .parse()
            .unwrap_or(StorageType::Database);

        let region = self.get_config_value("AWS_REGION", Some(crate::config::constants::DEFAULT_S3_REGION)).await
            .unwrap_or_else(|| crate::config::constants::DEFAULT_S3_REGION.to_string());
        let bucket = self.get_config_value("S3_BUCKET", Some(crate::config::constants::DEFAULT_S3_BUCKET)).await
            .unwrap_or_else(|| crate::config::constants::DEFAULT_S3_BUCKET.to_string());
        let key_prefix = self.get_config_value("S3_KEY_PREFIX", Some(crate::config::constants::DEFAULT_S3_KEY_PREFIX)).await
            .unwrap_or_else(|| crate::config::constants::DEFAULT_S3_KEY_PREFIX.to_string());
        let access_key_id = self.get_config_value("AWS_ACCESS_KEY_ID", None).await;
        let secret_access_key = self.get_config_value("AWS_SECRET_ACCESS_KEY", None).await;
        let session_token = self.get_config_value("AWS_SESSION_TOKEN", None).await;
        let endpoint_url = self.get_config_value("S3_ENDPOINT_URL", None).await;
        let force_path_style = self.get_config_value("S3_FORCE_PATH_STYLE", Some(if crate::config::constants::DEFAULT_S3_FORCE_PATH_STYLE {"true"} else {"false"})).await
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(crate::config::constants::DEFAULT_S3_FORCE_PATH_STYLE);
        let use_secret_manager = self.get_config_value("USE_AWS_SECRET_MANAGER", Some(if crate::config::constants::DEFAULT_S3_USE_SECRET_MANAGER {"true"} else {"false"})).await
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(crate::config::constants::DEFAULT_S3_USE_SECRET_MANAGER);
        let secret_name = self.get_config_value("AWS_SECRET_NAME", None).await;
        let timeout_seconds = self.get_config_value("S3_TIMEOUT_SECONDS", Some(&crate::config::constants::DEFAULT_S3_TIMEOUT_SECONDS.to_string())).await
            .and_then(|v| v.parse().ok())
            .unwrap_or(crate::config::constants::DEFAULT_S3_TIMEOUT_SECONDS);
        let max_retries = self.get_config_value("S3_MAX_RETRIES", Some(&crate::config::constants::DEFAULT_S3_MAX_RETRIES.to_string())).await
            .and_then(|v| v.parse().ok())
            .unwrap_or(crate::config::constants::DEFAULT_S3_MAX_RETRIES);

        let file_ttl_secs = self.get_config_value("FILE_TTL_SECS", Some(&crate::config::constants::DEFAULT_FILE_TTL_SECS.to_string())).await
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_FILE_TTL_SECS);
        let max_file_revisions = self.get_config_value("MAX_FILE_REVISIONS", Some(&crate::config::constants::DEFAULT_MAX_FILE_REVISIONS.to_string())).await
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(crate::config::constants::DEFAULT_MAX_FILE_REVISIONS);

        StorageConfig {
            storage_type,
            s3: super::settings::S3Config {
                region,
                bucket,
                key_prefix,
                access_key_id,
                secret_access_key,
                session_token,
                endpoint_url,
                force_path_style,
                use_secret_manager,
                secret_name,
                timeout_seconds,
                max_retries,
            },
            file_ttl_secs,
            max_file_revisions,
        }
    }

    /// Get logging configuration from secrets or environment
    pub async fn get_logging_config(&self) -> super::settings::LoggingConfig {
        use super::settings::LoggingConfig;

        let level = self.get_config_value("LOG_LEVEL", Some("info")).await
            .unwrap_or_else(|| "info".to_string());
        let file_logging = self.get_config_value("LOG_TO_FILE", Some("true")).await
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(true);
        let log_file = self.get_config_value("LOG_FILE", Some("logs/cosmic-sync-server.log")).await
            .unwrap_or_else(|| "logs/cosmic-sync-server.log".to_string());
        let max_file_size = self.get_config_value("LOG_MAX_FILE_SIZE", Some("10485760")).await // 10MB
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(10 * 1024 * 1024);
        let max_backups = self.get_config_value("LOG_MAX_BACKUPS", Some("5")).await
            .and_then(|b| b.parse::<usize>().ok())
            .unwrap_or(5);

        LoggingConfig {
            level,
            file_logging,
            log_file,
            max_file_size,
            max_backups,
        }
    }

    /// Get feature flags from secrets or environment
    pub async fn get_feature_flags(&self) -> super::settings::FeatureFlags {
        use super::settings::FeatureFlags;

        let test_mode = self.get_config_value("COSMIC_SYNC_TEST_MODE", Some("false")).await
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        let debug_mode = self.get_config_value("COSMIC_SYNC_DEBUG_MODE", Some("false")).await
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        let metrics_enabled = self.get_config_value("ENABLE_METRICS", Some("false")).await
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        let storage_encryption = self.get_config_value("STORAGE_ENCRYPTION", Some("true")).await
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(true);
        let request_validation = self.get_config_value("REQUEST_VALIDATION", Some("true")).await
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(true);

        FeatureFlags {
            test_mode,
            debug_mode,
            metrics_enabled,
            storage_encryption,
            request_validation,
        }
    }

    /// Load complete configuration
    pub async fn load_config(&self) -> super::settings::Config {
        info!("Loading configuration for environment: {:?}", self.environment);

        let server = self.get_server_config().await;
        let database = self.get_database_config().await;
        let storage = self.get_storage_config().await;
        let logging = self.get_logging_config().await;
        let features = self.get_feature_flags().await;
        let message_broker = super::settings::MessageBrokerConfig::load();

        info!("Configuration loaded successfully");

        super::settings::Config {
            server,
            database,
            storage,
            logging,
            features,
            message_broker,
        }
    }
} 