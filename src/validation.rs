//! Input validation utilities

// use std::collections::HashMap; // not used currently
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
}

impl ValidationResult {
    pub fn new() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
        }
    }
    
    pub fn add_error(&mut self, field: String, message: String) {
        self.is_valid = false;
        self.errors.push(ValidationError { field, message });
    }
}

/// Validates if a string is a valid hash format (64 hex characters)
pub fn validate_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())
}

/// Validates if a string is a valid UUID format
pub fn validate_uuid(uuid: &str) -> bool {
    uuid::Uuid::parse_str(uuid).is_ok()
}

/// Validates file path for safety
pub fn validate_file_path(path: &str) -> bool {
    !path.contains("..") && !path.starts_with("/") && !path.is_empty()
}

/// Validates device hash format
pub fn validate_device_hash(hash: &str) -> bool {
    validate_hash(hash)
}

/// Validates account hash format
pub fn validate_account_hash(hash: &str) -> bool {
    validate_hash(hash)
} 