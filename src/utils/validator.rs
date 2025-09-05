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

	const ENV_ALLOW_NUMERIC: &str = "WATCHER_FOLDER_ALLOW_NUMERIC";
	const ENV_WHITELIST: &str = "WATCHER_FOLDER_NUMERIC_SEGMENT_WHITELIST";
	const ENV_REGEX: &str = "WATCHER_FOLDER_NUMERIC_SEGMENT_REGEX";

	let allow_numeric = std::env::var(ENV_ALLOW_NUMERIC).unwrap_or_else(|_| "0".to_string()) == "1";
	if allow_numeric {
		return Ok(());
	}

	let whitelist_env = std::env::var(ENV_WHITELIST).unwrap_or_default();
	let whitelist: HashSet<String> = whitelist_env
		.split(',')
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty())
		.collect();

	let regex_opt = match std::env::var(ENV_REGEX) {
		Ok(pat) if !pat.trim().is_empty() => Regex::new(pat.trim()).ok(),
		_ => None,
	};

	for seg in folder.split('/') {
		if seg.is_empty() || seg == "~" || seg == "." || seg == ".." { continue; }
		let is_numeric_only = seg.chars().all(|c| c.is_ascii_digit());
		if is_numeric_only {
			let allowed = whitelist.contains(seg) || regex_opt.as_ref().map_or(false, |re| re.is_match(seg));
			if !allowed {
				return Err(format!("Watcher folder contains numeric-only segment '{}' which is not allowed (set {}=1 or whitelist/regex)", seg, ENV_ALLOW_NUMERIC));
			}
		}
	}

	Ok(())
}

/// Validate watcher folder path using options provided by caller (from Secrets/Config)
pub fn validate_watcher_folder_with_options(
    folder: &str,
    allow_numeric: bool,
    whitelist_csv: &str,
    regex_pattern: Option<&str>,
) -> Result<(), String> {
    use regex::Regex;
    use std::collections::HashSet;

    if allow_numeric { return Ok(()); }

    let whitelist: HashSet<String> = whitelist_csv
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let regex_opt = match regex_pattern {
        Some(pat) if !pat.trim().is_empty() => Regex::new(pat.trim()).ok(),
        _ => None,
    };

    for seg in folder.split('/') {
        if seg.is_empty() || seg == "~" || seg == "." || seg == ".." { continue; }
        let is_numeric_only = seg.chars().all(|c| c.is_ascii_digit());
        if is_numeric_only {
            let allowed = whitelist.contains(seg) || regex_opt.as_ref().map_or(false, |re| re.is_match(seg));
            if !allowed {
                return Err(format!(
                    "Watcher folder contains numeric-only segment '{}' which is not allowed (enable allow_numeric or whitelist/regex)",
                    seg
                ));
            }
        }
    }

    Ok(())
} 