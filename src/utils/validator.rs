/// Validate if a device ID is in the correct format
pub fn validate_device_hash(device_hash: &str) -> bool {
    // In a real implementation, this would check for a valid device ID format
    // For this example, we'll just ensure it's not empty
    !device_hash.is_empty()
}

/// Validate if a filename is valid
pub fn validate_filename(filename: &str) -> bool {
    // In a real implementation, this would check for valid filename characters
    // For this example, we'll just ensure it's not empty and doesn't contain invalid characters
    !filename.is_empty() && !filename.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
} 