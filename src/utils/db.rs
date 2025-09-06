use crate::storage::StorageError;

/// Validate mysql:// URL and return it (sqlx uses plain URL string)
pub fn parse_mysql_url(url: &str) -> Result<String, StorageError> {
    if !url.starts_with("mysql://") {
        return Err(StorageError::ConfigurationError(
            "Only MySQL URLs are supported".to_string(),
        ));
    }
    Ok(url.to_string())
}
