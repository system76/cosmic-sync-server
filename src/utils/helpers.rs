use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

// Simple memoization cache for hot path normalization
static NORMALIZE_CACHE: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::with_capacity(1024)));

/// Format a datetime for display
pub fn format_datetime(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Format a file size for human-readable display
pub fn format_file_size(size: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = KB * 1024;
    const GB: usize = MB * 1024;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} bytes", size)
    }
}

/// Normalize file path to preserve tilde (~) prefix for home directory
/// This keeps relative paths starting with ~ unchanged for consistent storage
/// and converts absolute paths to tilde-based paths when possible
pub fn normalize_path_preserve_tilde(path: &str) -> String {
    // Fast path: check cache first
    if path.len() <= 256 {
        // avoid caching excessively long keys
        if let Ok(cache) = NORMALIZE_CACHE.lock() {
            if let Some(v) = cache.get(path) {
                return v.clone();
            }
        }
    }

    // Handle null or empty paths
    if path.is_empty() {
        let v = "~/".to_string();
        if path.len() <= 256 {
            if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                cache.insert(path.to_string(), v.clone());
            }
        }
        return v;
    }

    // Handle whitespace-only paths
    let trimmed_path = path.trim();
    if trimmed_path.is_empty() {
        let v = "~/".to_string();
        if path.len() <= 256 {
            if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                cache.insert(path.to_string(), v.clone());
            }
        }
        return v;
    }

    // Handle paths that are already tilde-based
    if trimmed_path == "~" || trimmed_path == "~/." {
        let v = "~/".to_string();
        if path.len() <= 256 {
            if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                cache.insert(path.to_string(), v.clone());
            }
        }
        return v;
    }

    if trimmed_path.starts_with("~/") {
        // Keep the tilde prefix as-is for consistent relative path storage
        let v = trimmed_path.to_string();
        if path.len() <= 256 {
            if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                cache.insert(path.to_string(), v.clone());
            }
        }
        return v;
    }

    // Handle absolute paths
    if trimmed_path.starts_with('/') {
        // Special handling for /home/username patterns
        if trimmed_path.starts_with("/home/") {
            // Ensure we have enough characters for safe processing
            if trimmed_path.len() <= 6 {
                let v = "~".to_string();
                if path.len() <= 256 {
                    if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                        cache.insert(path.to_string(), v.clone());
                    }
                }
                return v;
            }

            // Look for the next slash after /home/
            let after_home = &trimmed_path[6..];
            if let Some(slash_pos) = after_home.find('/') {
                // Found pattern: /home/username/...
                let remaining_path = &after_home[slash_pos..];
                if remaining_path.is_empty() || remaining_path == "/" {
                    let v = "~".to_string();
                    if path.len() <= 256 {
                        if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                            cache.insert(path.to_string(), v.clone());
                        }
                    }
                    return v;
                } else {
                    let v = format!("~{}", remaining_path);
                    if path.len() <= 256 {
                        if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                            cache.insert(path.to_string(), v.clone());
                        }
                    }
                    return v;
                }
            } else {
                // Pattern: /home/username (no trailing path)
                let v = "~".to_string();
                if path.len() <= 256 {
                    if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                        cache.insert(path.to_string(), v.clone());
                    }
                }
                return v;
            }
        } else {
            // Other absolute paths: /path -> ~/path (but handle root specially)
            if trimmed_path == "/" {
                let v = "~".to_string();
                if path.len() <= 256 {
                    if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                        cache.insert(path.to_string(), v.clone());
                    }
                }
                return v;
            } else {
                let v = format!("~{}", trimmed_path);
                if path.len() <= 256 {
                    if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                        cache.insert(path.to_string(), v.clone());
                    }
                }
                return v;
            }
        }
    }

    // Handle relative paths starting with ./
    if trimmed_path.starts_with("./") {
        if trimmed_path.len() <= 2 {
            let v = "~".to_string();
            if path.len() <= 256 {
                if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                    cache.insert(path.to_string(), v.clone());
                }
            }
            return v;
        } else {
            let after_dot = &trimmed_path[2..];
            if after_dot.is_empty() {
                let v = "~".to_string();
                if path.len() <= 256 {
                    if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                        cache.insert(path.to_string(), v.clone());
                    }
                }
                return v;
            } else {
                let v = format!("~/{}", after_dot);
                if path.len() <= 256 {
                    if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                        cache.insert(path.to_string(), v.clone());
                    }
                }
                return v;
            }
        }
    }

    // Handle hidden config directories
    if trimmed_path.starts_with(".config/")
        || trimmed_path.starts_with(".local/")
        || trimmed_path.starts_with(".cache/")
    {
        let v = format!("~/{}", trimmed_path);
        if path.len() <= 256 {
            if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                cache.insert(path.to_string(), v.clone());
            }
        }
        return v;
    }

    // Handle single filename without path
    if !trimmed_path.contains('/') {
        let v = format!("~/{}", trimmed_path);
        if path.len() <= 256 {
            if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
                cache.insert(path.to_string(), v.clone());
            }
        }
        return v;
    }

    // Handle other relative paths
    let v = format!("~/{}", trimmed_path);
    if path.len() <= 256 {
        if let Ok(mut cache) = NORMALIZE_CACHE.lock() {
            cache.insert(path.to_string(), v.clone());
        }
    }
    return v;
}

/// Check if a path is a home directory relative path
pub fn is_home_relative_path(path: &str) -> bool {
    path.starts_with("~/") || path == "~"
}

/// Convert home relative path to a consistent format for storage
/// This ensures that ~/Documents/file.txt is always stored as ~/Documents/file.txt
pub fn canonicalize_home_path(path: &str) -> String {
    if path.starts_with("~/") {
        // Ensure consistent format: ~/path/to/file
        path.to_string()
    } else if path == "~" {
        // Handle edge case of just ~
        "~".to_string()
    } else {
        // For non-home paths, return as-is
        path.to_string()
    }
}

/// Test function for path normalization - returns (success, result, error_msg)
pub fn test_normalize_path_preserve_tilde(path: &str) -> (bool, String, String) {
    match std::panic::catch_unwind(|| normalize_path_preserve_tilde(path)) {
        Ok(result) => (true, result, String::new()),
        Err(e) => {
            let error_msg = if let Some(s) = e.downcast_ref::<&str>() {
                format!("Panic: {}", s)
            } else if let Some(s) = e.downcast_ref::<String>() {
                format!("Panic: {}", s)
            } else {
                "Unknown panic occurred".to_string()
            };
            (false, "~".to_string(), error_msg)
        }
    }
}
