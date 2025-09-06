use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
// remove unused fs helpers here (file storage is DB/S3 backed)

// AWS SDK imports
use aws_config::{meta::region::RegionProviderChain, BehaviorVersion};
use aws_sdk_s3::operation::create_bucket::CreateBucketError;
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::operation::head_bucket::HeadBucketError;
use aws_sdk_s3::operation::head_object::HeadObjectError;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{BucketLocationConstraint, CreateBucketConfiguration};
use aws_sdk_s3::{config::Credentials, Client as S3Client};
use aws_types::region::Region;
use tokio::sync::OnceCell;

use crate::config::settings::S3Config;
use crate::storage::mysql::MySqlStorage;
use crate::storage::mysql_file::MySqlFileExt;
use crate::storage::{FileStorage, Result, StorageError, StorageMetrics};

/// Database-based file storage implementation
/// Stores files as BLOB in the database
pub struct DatabaseFileStorage {
    mysql_storage: Arc<MySqlStorage>,
}

impl DatabaseFileStorage {
    pub async fn new() -> Result<Self> {
        // Create MySQL storage from configuration
        let config = crate::config::settings::Config::load();

        match Self::create_mysql_storage_from_config(&config).await {
            Ok(mysql_storage) => {
                info!("DatabaseFileStorage initialized with MySQL from config");
                Ok(Self {
                    mysql_storage: Arc::new(mysql_storage),
                })
            }
            Err(e) => {
                error!("Failed to initialize DatabaseFileStorage with MySQL: {}", e);
                Err(e)
            }
        }
    }

    pub fn with_mysql_storage(mysql_storage: Arc<MySqlStorage>) -> Self {
        Self { mysql_storage }
    }

    async fn create_mysql_storage_from_config(
        config: &crate::config::settings::Config,
    ) -> Result<MySqlStorage> {
        let db_url = config.database.url();
        let url = crate::utils::db::parse_mysql_url(&db_url)?;
        MySqlStorage::new_with_url(&url).await
    }
}

#[async_trait]
impl FileStorage for DatabaseFileStorage {
    async fn store_file_data(&self, file_id: u64, data: Vec<u8>) -> Result<String> {
        debug!(
            "Storing file data in database: file_id={}, size={}",
            file_id,
            data.len()
        );

        // Use the existing MySQL file storage implementation
        self.mysql_storage.store_file_data(file_id, data).await?;

        Ok(format!("db:{}", file_id))
    }

    async fn store_file_data_with_options(
        &self,
        file_id: u64,
        data: Vec<u8>,
        _compress: bool,
    ) -> Result<String> {
        self.store_file_data(file_id, data).await
    }

    async fn get_file_data(&self, file_id: u64) -> Result<Option<Vec<u8>>> {
        debug!("Retrieving file data from database: file_id={}", file_id);

        // Use the existing MySQL file storage implementation
        self.mysql_storage.get_file_data(file_id).await
    }

    async fn delete_file_data(&self, file_id: u64) -> Result<()> {
        debug!("Deleting file data from database: file_id={}", file_id);

        // For database storage, we don't delete the blob data directly
        // This is handled by the file deletion logic in the main Storage trait
        // For now, we just log the operation
        info!(
            "Database file data deletion for file_id={} handled by main storage logic",
            file_id
        );

        Ok(())
    }

    async fn batch_delete_file_data(&self, file_ids: Vec<u64>) -> Result<Vec<(u64, bool)>> {
        let mut results = Vec::with_capacity(file_ids.len());
        for file_id in file_ids {
            match self.delete_file_data(file_id).await {
                Ok(_) => results.push((file_id, true)),
                Err(_) => results.push((file_id, false)),
            }
        }
        Ok(results)
    }

    async fn file_data_exists(&self, file_id: u64) -> Result<bool> {
        debug!(
            "Checking if file data exists in database: file_id={}",
            file_id
        );

        // Check if file data exists in database
        match self.mysql_storage.get_file_data(file_id).await? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    async fn get_file_size(&self, file_id: u64) -> Result<Option<u64>> {
        debug!("Getting file size from database: file_id={}", file_id);

        // Get file data and return its size
        match self.mysql_storage.get_file_data(file_id).await? {
            Some(data) => Ok(Some(data.len() as u64)),
            None => Ok(None),
        }
    }

    async fn health_check(&self) -> Result<()> {
        debug!("Database file storage health check");

        // Try to perform a simple database operation
        match self.mysql_storage.get_file_data(0).await {
            Ok(_) => Ok(()),
            Err(StorageError::NotFound(_)) => Ok(()), // Not found is acceptable for health check
            Err(e) => Err(e),
        }
    }

    fn storage_type(&self) -> &'static str {
        "database"
    }

    async fn get_metrics(&self) -> Result<StorageMetrics> {
        Ok(StorageMetrics::default())
    }

    async fn cleanup_orphaned_data(&self) -> Result<u64> {
        // TODO: Implement S3 orphaned data cleanup
        Ok(0)
    }
}

/// S3-based file storage implementation
/// Stores files in Amazon S3 or S3-compatible storage
pub struct S3FileStorage {
    config: S3Config,
    client: OnceCell<Result<S3Client>>, // Lazy-initialized S3 client
    bucket_ready: OnceCell<Result<()>>, // Ensures bucket exists once
}

impl S3FileStorage {
    pub async fn new(config: &S3Config) -> Result<Self> {
        debug!(
            "Initializing S3 file storage (lazy) with bucket: {}",
            config.bucket
        );
        Ok(Self {
            config: config.clone(),
            client: OnceCell::new(),
            bucket_ready: OnceCell::new(),
        })
    }

    async fn create_s3_client(config: &S3Config) -> Result<S3Client> {
        // Set up AWS configuration
        let mut aws_config_builder = aws_config::defaults(BehaviorVersion::latest());

        // Configure region
        aws_config_builder = aws_config_builder.region(Region::new(config.region.clone()));

        // Configure credentials if provided
        if let (Some(access_key_id), Some(secret_access_key)) =
            (&config.access_key_id, &config.secret_access_key)
        {
            let creds = Credentials::new(
                access_key_id,
                secret_access_key,
                config.session_token.clone(),
                None,
                "cosmic-sync-s3",
            );

            aws_config_builder = aws_config_builder.credentials_provider(creds);
        }

        let aws_config = aws_config_builder.load().await;

        // Create S3 client with custom configuration
        let mut s3_config_builder = aws_sdk_s3::config::Builder::from(&aws_config);

        // Configure custom endpoint (for MinIO or other S3-compatible services)
        if let Some(endpoint_url) = &config.endpoint_url {
            s3_config_builder = s3_config_builder.endpoint_url(endpoint_url);
        }

        // Configure path style addressing (important for MinIO)
        if config.force_path_style {
            s3_config_builder = s3_config_builder.force_path_style(true);
        }

        let s3_config = s3_config_builder.build();
        let client = S3Client::from_conf(s3_config);

        Ok(client)
    }

    async fn get_client(&self) -> Result<&S3Client> {
        let result_ref = self
            .client
            .get_or_init(|| async {
                match Self::create_s3_client(&self.config).await {
                    Ok(c) => Ok(c),
                    Err(e) => Err(e),
                }
            })
            .await;
        match result_ref {
            Ok(client) => Ok(client),
            Err(e) => Err(StorageError::S3Error(format!(
                "Failed to initialize S3 client: {}",
                e
            ))),
        }
    }

    async fn ensure_bucket_exists_once(&self) -> Result<()> {
        let res_ref = self
            .bucket_ready
            .get_or_init(|| async { self.ensure_bucket_exists().await })
            .await;
        match res_ref {
            Ok(()) => Ok(()),
            Err(e) => Err(StorageError::S3Error(format!(
                "Bucket ensure failed: {}",
                e
            ))),
        }
    }

    /// Ensures that the configured S3 bucket exists, creating it if necessary
    async fn ensure_bucket_exists(&self) -> Result<()> {
        debug!("Checking if bucket exists: {}", self.config.bucket);

        // First try to check if bucket exists
        let client = self.get_client().await?;
        match client
            .head_bucket()
            .bucket(&self.config.bucket)
            .send()
            .await
        {
            Ok(_) => {
                debug!("Bucket exists: {}", self.config.bucket);
                Ok(())
            }
            Err(e) => {
                // Check if it's a 404 error (bucket doesn't exist)
                if let aws_sdk_s3::error::SdkError::ServiceError(service_err) = &e {
                    if let HeadBucketError::NotFound(_) = service_err.err() {
                        info!("Bucket does not exist, creating: {}", self.config.bucket);
                        return self.create_bucket().await;
                    }
                }

                // Other errors (permission issues, network problems, etc.)
                error!("Failed to check bucket existence: {}", e);
                Err(StorageError::S3Error(format!(
                    "Failed to check bucket existence: {}",
                    e
                )))
            }
        }
    }

    /// Creates the S3 bucket with proper region handling
    async fn create_bucket(&self) -> Result<()> {
        info!("Creating S3 bucket: {}", self.config.bucket);

        let client = self.get_client().await?;
        let mut create_bucket_request = client.create_bucket().bucket(&self.config.bucket);

        // For AWS S3, we need to set CreateBucketConfiguration for regions other than us-east-1
        // For MinIO and other S3-compatible services, this might not be necessary
        if self.config.region != "us-east-1" && !self.is_minio() {
            let constraint = BucketLocationConstraint::from(self.config.region.as_str());
            let config = CreateBucketConfiguration::builder()
                .location_constraint(constraint)
                .build();
            create_bucket_request = create_bucket_request.create_bucket_configuration(config);
        }

        match create_bucket_request.send().await {
            Ok(_) => {
                info!("Successfully created bucket: {}", self.config.bucket);
                Ok(())
            }
            Err(e) => {
                // Handle specific bucket creation errors
                if let aws_sdk_s3::error::SdkError::ServiceError(service_err) = &e {
                    match service_err.err() {
                        CreateBucketError::BucketAlreadyExists(_) => {
                            warn!(
                                "Bucket already exists (owned by another account): {}",
                                self.config.bucket
                            );
                            Err(StorageError::S3Error(format!(
                                "Bucket already exists: {}",
                                self.config.bucket
                            )))
                        }
                        CreateBucketError::BucketAlreadyOwnedByYou(_) => {
                            info!(
                                "Bucket already exists and is owned by you: {}",
                                self.config.bucket
                            );
                            Ok(()) // This is fine, race condition resolved
                        }
                        _ => {
                            error!("Failed to create bucket {}: {}", self.config.bucket, e);
                            Err(StorageError::S3Error(format!(
                                "Failed to create bucket: {}",
                                e
                            )))
                        }
                    }
                } else {
                    error!("Failed to create bucket {}: {}", self.config.bucket, e);
                    Err(StorageError::S3Error(format!(
                        "Failed to create bucket: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Detects if we're using MinIO based on endpoint URL
    fn is_minio(&self) -> bool {
        self.config
            .endpoint_url
            .as_ref()
            .map(|url| {
                url.contains("minio") || url.contains("localhost") || url.contains("127.0.0.1")
            })
            .unwrap_or(false)
    }

    /// Helper method to perform the actual S3 upload
    async fn upload_to_s3(&self, s3_key: &str, stream: ByteStream) -> Result<String> {
        self.ensure_bucket_exists_once().await?;
        let client = self.get_client().await?;
        match client
            .put_object()
            .bucket(&self.config.bucket)
            .key(s3_key)
            .body(stream)
            .send()
            .await
        {
            Ok(_) => {
                info!("Successfully stored file in S3: {}", s3_key);
                Ok(format!("s3:{}", s3_key))
            }
            Err(e) => {
                error!("Failed to store file in S3: {}", e);
                Err(StorageError::S3Error(format!(
                    "Failed to store file in S3: {}",
                    e
                )))
            }
        }
    }

    fn generate_s3_key(&self, file_id: u64) -> String {
        format!("{}{}", self.config.key_prefix, file_id)
    }
}

#[async_trait]
impl FileStorage for S3FileStorage {
    async fn store_file_data(&self, file_id: u64, data: Vec<u8>) -> Result<String> {
        let s3_key = self.generate_s3_key(file_id);
        info!(
            "Storing file data in S3: bucket={}, key={}, size={}",
            self.config.bucket,
            s3_key,
            data.len()
        );

        // Create ByteStream from data
        let stream = ByteStream::from(data.clone());

        // First attempt to upload
        match self.upload_to_s3(&s3_key, stream).await {
            Ok(result) => Ok(result),
            Err(e) => {
                // Check if error might be due to missing bucket
                if let StorageError::S3Error(ref msg) = e {
                    if msg.contains("NoSuchBucket") || msg.contains("BucketNotFound") {
                        warn!(
                            "Bucket not found during upload, attempting to create bucket and retry"
                        );

                        // Try to create bucket and retry upload
                        if let Err(create_err) = self.create_bucket().await {
                            error!("Failed to create bucket during retry: {}", create_err);
                            return Err(create_err);
                        }

                        info!("Retrying upload after bucket creation");
                        let retry_stream = ByteStream::from(data);
                        return self.upload_to_s3(&s3_key, retry_stream).await;
                    }
                }

                Err(e)
            }
        }
    }

    async fn get_file_data(&self, file_id: u64) -> Result<Option<Vec<u8>>> {
        let s3_key = self.generate_s3_key(file_id);
        debug!(
            "Retrieving file data from S3: bucket={}, key={}",
            self.config.bucket, s3_key
        );

        // Download from S3
        let client = self.get_client().await?;
        match client
            .get_object()
            .bucket(&self.config.bucket)
            .key(&s3_key)
            .send()
            .await
        {
            Ok(response) => {
                // Collect bytes from the response body
                match response.body.collect().await {
                    Ok(bytes) => {
                        let data = bytes.into_bytes().to_vec();
                        debug!(
                            "Successfully retrieved file from S3: {}, size={}",
                            s3_key,
                            data.len()
                        );
                        Ok(Some(data))
                    }
                    Err(e) => {
                        error!("Failed to read S3 response body: {}", e);
                        Err(StorageError::S3Error(format!(
                            "Failed to read response: {}",
                            e
                        )))
                    }
                }
            }
            Err(e) => {
                // Check if it's a 404 error
                if let aws_sdk_s3::error::SdkError::ServiceError(service_err) = &e {
                    if let GetObjectError::NoSuchKey(_) = service_err.err() {
                        debug!("File not found in S3: {}", s3_key);
                        return Ok(None);
                    }
                }

                error!("Failed to get file from S3: {}", e);
                Err(StorageError::S3Error(format!(
                    "Failed to download file: {}",
                    e
                )))
            }
        }
    }

    async fn delete_file_data(&self, file_id: u64) -> Result<()> {
        let s3_key = self.generate_s3_key(file_id);
        debug!(
            "Deleting file data from S3: bucket={}, key={}",
            self.config.bucket, s3_key
        );

        // Delete from S3
        let client = self.get_client().await?;
        match client
            .delete_object()
            .bucket(&self.config.bucket)
            .key(&s3_key)
            .send()
            .await
        {
            Ok(_) => {
                info!("Successfully deleted file from S3: {}", s3_key);
                Ok(())
            }
            Err(e) => {
                // S3 delete operations are idempotent - not found is not an error
                // Just log the error and continue
                warn!("S3 delete operation completed with warning: {}", e);
                Ok(())
            }
        }
    }

    async fn file_data_exists(&self, file_id: u64) -> Result<bool> {
        let s3_key = self.generate_s3_key(file_id);
        debug!(
            "Checking if file data exists in S3: bucket={}, key={}",
            self.config.bucket, s3_key
        );

        // Use HEAD request to check existence
        let client = self.get_client().await?;
        match client
            .head_object()
            .bucket(&self.config.bucket)
            .key(&s3_key)
            .send()
            .await
        {
            Ok(_) => {
                debug!("File exists in S3: {}", s3_key);
                Ok(true)
            }
            Err(e) => {
                // Check if it's a 404 error
                if let aws_sdk_s3::error::SdkError::ServiceError(service_err) = &e {
                    if let HeadObjectError::NotFound(_) = service_err.err() {
                        debug!("File not found in S3: {}", s3_key);
                        return Ok(false);
                    }
                }

                error!("Failed to check file existence in S3: {}", e);
                Err(StorageError::S3Error(format!(
                    "Failed to check file existence: {}",
                    e
                )))
            }
        }
    }

    async fn get_file_size(&self, file_id: u64) -> Result<Option<u64>> {
        let s3_key = self.generate_s3_key(file_id);
        debug!(
            "Getting file size from S3: bucket={}, key={}",
            self.config.bucket, s3_key
        );

        // Use HEAD request to get file metadata including size
        let client = self.get_client().await?;
        match client
            .head_object()
            .bucket(&self.config.bucket)
            .key(&s3_key)
            .send()
            .await
        {
            Ok(response) => {
                let size = response.content_length().unwrap_or(0) as u64;
                debug!("File size in S3: {} = {} bytes", s3_key, size);
                Ok(Some(size))
            }
            Err(e) => {
                // Check if it's a 404 error
                if let aws_sdk_s3::error::SdkError::ServiceError(service_err) = &e {
                    if let HeadObjectError::NotFound(_) = service_err.err() {
                        debug!("File not found in S3: {}", s3_key);
                        return Ok(None);
                    }
                }

                error!("Failed to get file size from S3: {}", e);
                Err(StorageError::S3Error(format!(
                    "Failed to get file size: {}",
                    e
                )))
            }
        }
    }

    async fn health_check(&self) -> Result<()> {
        debug!(
            "S3 file storage health check for bucket: {}",
            self.config.bucket
        );

        // First check if bucket exists
        let client = self.get_client().await?;
        match client
            .head_bucket()
            .bucket(&self.config.bucket)
            .send()
            .await
        {
            Ok(_) => {
                debug!("Bucket health check passed: {}", self.config.bucket);
            }
            Err(e) => {
                error!("Bucket health check failed: {}", e);
                return Err(StorageError::S3Error(format!(
                    "Bucket health check failed: {}",
                    e
                )));
            }
        }

        // Then try to list objects to verify read access
        let client = self.get_client().await?;
        match client
            .list_objects_v2()
            .bucket(&self.config.bucket)
            .max_keys(1)
            .send()
            .await
        {
            Ok(_) => {
                debug!("S3 health check successful - bucket access verified");
                Ok(())
            }
            Err(e) => {
                error!("S3 health check failed - list objects failed: {}", e);
                Err(StorageError::S3Error(format!("Health check failed: {}", e)))
            }
        }
    }

    // Stub implementations for missing methods
    async fn store_file_data_with_options(
        &self,
        file_id: u64,
        data: Vec<u8>,
        _compress: bool,
    ) -> Result<String> {
        // For now, ignore compression option and use existing method
        self.store_file_data(file_id, data).await
    }

    async fn batch_delete_file_data(&self, file_ids: Vec<u64>) -> Result<Vec<(u64, bool)>> {
        let mut results = Vec::new();
        for file_id in file_ids {
            match self.delete_file_data(file_id).await {
                Ok(_) => results.push((file_id, true)),
                Err(_) => results.push((file_id, false)),
            }
        }
        Ok(results)
    }

    async fn get_metrics(&self) -> Result<StorageMetrics> {
        Ok(StorageMetrics {
            total_queries: 0,
            successful_queries: 0,
            failed_queries: 0,
            average_query_time_ms: 0.0,
            active_connections: 0,
            idle_connections: 0,
            cache_hits: 0,
            cache_misses: 0,
        })
    }

    async fn cleanup_orphaned_data(&self) -> Result<u64> {
        // TODO: Implement S3 orphaned data cleanup
        Ok(0)
    }

    fn storage_type(&self) -> &'static str {
        "s3"
    }
}
