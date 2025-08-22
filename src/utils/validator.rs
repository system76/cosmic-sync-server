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

/// Validate watcher folder path to reject numeric-only segments unless whitelisted
/// Env controls (optional):
/// - WATCHER_FOLDER_ALLOW_NUMERIC=1 to disable this validation entirely
/// - WATCHER_FOLDER_NUMERIC_SEGMENT_WHITELIST=comma,separated,segments
/// - WATCHER_FOLDER_NUMERIC_SEGMENT_REGEX=^\d{4}$ (regex for allowed segments)
pub fn validate_watcher_folder(folder: &str) -> Result<(), String> {
	use regex::Regex;
	use std::collections::HashSet;

	let allow_numeric = std::env::var("WATCHER_FOLDER_ALLOW_NUMERIC").unwrap_or_else(|_| "0".to_string()) == "1";
	if allow_numeric {
		return Ok(());
	}

	let whitelist_env = std::env::var("WATCHER_FOLDER_NUMERIC_SEGMENT_WHITELIST").unwrap_or_default();
	let whitelist: HashSet<String> = whitelist_env
		.split(',')
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty())
		.collect();

	let regex_opt = match std::env::var("WATCHER_FOLDER_NUMERIC_SEGMENT_REGEX") {
		Ok(pat) if !pat.trim().is_empty() => Regex::new(pat.trim()).ok(),
		_ => None,
	};

	for seg in folder.split('/') {
		if seg.is_empty() || seg == "~" || seg == "." || seg == ".." { continue; }
		let is_numeric_only = seg.chars().all(|c| c.is_ascii_digit());
		if is_numeric_only {
			let allowed = whitelist.contains(seg) || regex_opt.as_ref().map_or(false, |re| re.is_match(seg));
			if !allowed {
				return Err(format!("Watcher folder contains numeric-only segment '{}' which is not allowed (set WATCHER_FOLDER_ALLOW_NUMERIC=1 or whitelist/regex)", seg));
			}
		}
	}

	Ok(())
} 