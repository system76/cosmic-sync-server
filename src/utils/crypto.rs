use uuid::Uuid;
use sha2::{Sha256, Digest};
use rand::{Rng, rngs::OsRng, RngCore};
use hex;

/// Generate a random encryption key
pub fn generate_encryption_key() -> String {
    // In a real implementation, this would use a proper cryptographic library
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    hex::encode(key)
}

/// Generate a hash for an account
pub fn generate_account_hash(account_hash: &str) -> String {
    // 실제 계정 해시 생성
    sha256(account_hash)
}

/// User ID and email로 계정 해시 생성
pub fn generate_account_hash_from_email(user_id: &str, email: &str) -> String {
    let input = format!("{}:{}", user_id, email);
    sha256_as_string(&input)
}

/// 이메일만으로 계정 해시 생성 (클라이언트와 호환성 유지)
pub fn generate_account_hash_from_email_only(email: &str) -> String {
    sha256_as_string(email)
}

/// 클라이언트와 동일한 방식으로 계정 해시 생성 테스트
pub fn test_account_hash_generation(email: &str, name: &str, user_id: &str) {
    use tracing::info;
    
    // 다양한 방식으로 해시 생성
    let hash1 = sha256_as_string(email); // 이메일만
    let hash2 = sha256_as_string(&format!("{}:{}", user_id, email)); // user_id:email
    let hash3 = sha256_as_string(&format!("{}:{}", email, name)); // email:name
    let hash4 = sha256_as_string(&format!("{}:{}:{}", user_id, email, name)); // user_id:email:name
    let hash5 = sha256_as_string(&format!("{}:{}", name, email)); // name:email
    let hash6 = sha256_as_string(&user_id); // user_id만
    let hash7 = sha256_as_string(&format!("{}@system76.com", name.to_lowercase().replace(" ", ""))); // 추측: 이름 기반 이메일
    
    info!("🔐 Testing account hash generation:");
    info!("  Email: {}, Name: {}, UserID: {}", email, name, user_id);
    info!("  Hash from email only: {}", hash1);
    info!("  Hash from user_id:email: {}", hash2);
    info!("  Hash from email:name: {}", hash3);
    info!("  Hash from user_id:email:name: {}", hash4);
    info!("  Hash from name:email: {}", hash5);
    info!("  Hash from user_id only: {}", hash6);
    info!("  Hash from name-based email: {}", hash7);
    info!("  Target client hash: 209f313bf330cf40fe89fae938babbeba7ec95d31237f77cf19de418c0d50a0a");
}

/// Generate device hash from user ID and registration timestamp
pub fn generate_device_hash(user_id: &str, registered_at: &str) -> String {
    let input = format!("{}:{}", user_id, registered_at);
    sha256_as_string(&input)
}

/// Generate file ID from user, filename and file hash
pub fn generate_file_id(user_id: &str, filename: &str, file_hash: &str) -> u64 {
    // 현재 타임스탬프 추가 (나노초 정밀도)
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let timestamp_nanos = now.as_nanos();
    
    // 랜덤 요소 추가 (16비트)
    let random_part: u16 = rand::thread_rng().gen();
    
    // 원래 입력에 타임스탬프와 랜덤 값 추가
    let input = format!("{}:{}:{}:{}:{}", 
                        user_id, 
                        filename, 
                        file_hash, 
                        timestamp_nanos,
                        random_part);
    
    let hash_str = sha256_as_string_truncated(&input, 8); // 8바이트(64비트) 해시값으로 제한
    
    // 해시 문자열을 u64로 변환
    match u64::from_str_radix(&hash_str, 16) {
        Ok(value) => {
            // i64 범위 내로 제한 (최대값: 9,223,372,036,854,775,807)
            // 이렇게 하면 클라이언트에서 i64로 변환할 때 오류가 발생하지 않음
            if value > i64::MAX as u64 {
                value & (i64::MAX as u64)
            } else {
                value
            }
        },
        Err(_) => {
            // 변환 실패 시 대체값(현재 시간 기반)
            let value = now.as_secs() ^ (random_part as u64) ^ now.subsec_nanos() as u64;
            // 대체값도 i64 범위 내로 제한
            if value > i64::MAX as u64 {
                value & (i64::MAX as u64)
            } else {
                value
            }
        }
    }
}

/// Create a SHA256 hash of a string
pub fn sha256(input: &str) -> String {
    sha256_as_string(input)
}

/// SHA256 해시를 16진수 문자열로 반환
pub fn sha256_as_string(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    
    hex::encode(result)
}

/// SHA256 해시를 지정된 길이만큼 잘라서 반환
pub fn sha256_as_string_truncated(input: &str, length: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    
    hex::encode(&result[..length])
} 