fn main() {
    let test_paths = vec![
        "~/user/test_upload.txt", 
        "~/test_upload.txt",
        ".config/cosmic/test_upload.txt",
        "test_upload.txt",
        "./test_upload.txt",
        "~/.config/cosmic/test_upload.txt"
    ];
    
    println!("=== 경로 정규화 테스트 ===");
    for path in &test_paths {
        let normalized = normalize_path_preserve_tilde(path);
        let filename = get_filename(path);
        let final_path = extract_directory_path(&normalized, filename);
        
        println!("\n원본 경로: '{}'", path);
        println!("정규화된 경로: '{}'", normalized);
        println!("추출된 파일명: '{}'", filename);
        println!("최종 디렉토리 경로: '{}'", final_path);
    }
}

fn normalize_path_preserve_tilde(path: &str) -> String {
    if path.starts_with("~/") || path == "~" {
        // Keep the tilde prefix as-is for consistent relative path storage
        path.to_string()
    } else if path.starts_with("/") {
        // Convert absolute paths to tilde paths by removing leading slash and adding ~/
        if path.len() > 1 {
            format!("~{}", path)
        } else {
            "~".to_string()
        }
    } else if path.starts_with("./") {
        // ./으로 시작하는 상대 경로를 ~/로 시작하는 경로로 변환
        format!("~/{}", &path[2..])
    } else if path.starts_with(".config/") || path.starts_with(".local/") || path.starts_with(".cache/") {
        // 숨겨진 설정 디렉토리는 ~/로 시작하는 경로로 변환
        format!("~/{}", path)
    } else {
        // 기타 상대 경로는 ~/path 형태로 변환
        if path.is_empty() {
            "~".to_string()
        } else {
            format!("~/{}", path)
        }
    }
}

fn get_filename(path: &str) -> &str {
    match path.rfind('/') {
        Some(pos) => &path[pos+1..],
        None => path,
    }
}

fn extract_directory_path(path: &str, filename: &str) -> String {
    if path.ends_with(&format!("/{}", filename)) {
        // 경로가 이미 파일명으로 끝나는 경우, 파일명 부분 제거
        let parent_path = path.strip_suffix(&format!("/{}", filename)).unwrap_or(path);
        parent_path.to_string()
    } else if path == filename {
        // 경로가 파일명과 같은 경우 (경로 구분자 없음)
        "~".to_string()
    } else {
        // 그대로 반환
        path.to_string()
    }
}