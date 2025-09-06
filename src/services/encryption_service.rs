use crate::storage::{Storage, StorageError};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

const ENCRYPTION_KEY_LENGTH: usize = 32;

/// Service for managing encryption operations
#[derive(Clone)]
pub struct EncryptionService {
    storage: Arc<dyn Storage>,
}

impl EncryptionService {
    /// Create a new EncryptionService instance
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self { storage }
    }

    /// 암호화 키 요청 처리 (이전 버전 호환용)
    pub async fn request_encryption_key(
        &self,
        account_hash: &str,
        device_hash: &str,
    ) -> Result<String, StorageError> {
        debug!(
            "Processing encryption key request for account: {}, device: {}",
            account_hash, device_hash
        );
        self.get_or_create_key(account_hash).await
    }

    /// Get or create encryption key for account
    pub async fn get_or_create_key(&self, account_hash: &str) -> Result<String, StorageError> {
        debug!(
            "Getting or creating encryption key for account: {}",
            account_hash
        );

        match self.storage.get_encryption_key(account_hash).await {
            Ok(Some(key)) => {
                info!(
                    "Found existing encryption key for account: {}",
                    account_hash
                );
                Ok(key)
            }
            Ok(None) => {
                // 키가 없으면 새로 생성
                info!(
                    "No encryption key found for account: {}, generating new key",
                    account_hash
                );
                let new_key = self.generate_encryption_key();

                match self
                    .storage
                    .store_encryption_key(account_hash, &new_key)
                    .await
                {
                    Ok(_) => {
                        info!(
                            "Generated and stored new encryption key for account: {}",
                            account_hash
                        );
                        Ok(new_key)
                    }
                    Err(e) => {
                        error!("Failed to store encryption key: {}", e);
                        Err(e)
                    }
                }
            }
            Err(e) => {
                error!("Error retrieving encryption key: {}", e);
                Err(e)
            }
        }
    }

    /// 암호화 키 업데이트 처리 (이전 버전 호환용)
    pub async fn update_encryption_key(
        &self,
        account_hash: &str,
        device_hash: &str,
        new_key: &str,
    ) -> Result<(), StorageError> {
        debug!(
            "Processing encryption key update for account: {}, device: {}",
            account_hash, device_hash
        );
        self.update_key(account_hash, new_key).await
    }

    /// Update encryption key for account
    pub async fn update_key(&self, account_hash: &str, new_key: &str) -> Result<(), StorageError> {
        debug!("Updating encryption key for account: {}", account_hash);

        // 키 유효성 검사
        if new_key.len() < ENCRYPTION_KEY_LENGTH {
            warn!("Invalid encryption key length: {}", new_key.len());
            return Err(StorageError::ValidationError(format!(
                "Encryption key must be at least {} characters",
                ENCRYPTION_KEY_LENGTH
            )));
        }

        match self
            .storage
            .store_encryption_key(account_hash, new_key)
            .await
        {
            Ok(_) => {
                info!("Updated encryption key for account: {}", account_hash);
                Ok(())
            }
            Err(e) => {
                error!("Failed to update encryption key: {}", e);
                Err(e)
            }
        }
    }

    /// 랜덤 암호화 키 생성
    fn generate_encryption_key(&self) -> String {
        let key: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(ENCRYPTION_KEY_LENGTH)
            .map(char::from)
            .collect();

        debug!("Generated new encryption key");
        key
    }
}
