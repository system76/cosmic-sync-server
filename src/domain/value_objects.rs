// Domain value objects - Immutable objects that represent concepts

use crate::{
    domain::ValueObject,
    error::{Result, SyncError},
};
use serde::{Deserialize, Serialize};

/// 이메일 주소 값 객체
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmailAddress(String);

impl EmailAddress {
    /// 새 이메일 주소 생성
    pub fn new(email: String) -> Result<Self> {
        let email_obj = Self(email);
        email_obj.validate()?;
        Ok(email_obj)
    }

    /// 이메일 주소 문자열 반환
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 도메인 부분 추출
    pub fn domain(&self) -> Option<&str> {
        self.0.split('@').nth(1)
    }

    /// 로컬 부분 추출
    pub fn local_part(&self) -> Option<&str> {
        self.0.split('@').next()
    }
}

impl ValueObject for EmailAddress {
    fn validate(&self) -> Result<()> {
        if self.0.is_empty() {
            return Err(SyncError::Validation("Email cannot be empty".to_string()));
        }

        if !self.0.contains('@') {
            return Err(SyncError::Validation("Email must contain @".to_string()));
        }

        let parts: Vec<&str> = self.0.split('@').collect();
        if parts.len() != 2 {
            return Err(SyncError::Validation(
                "Email must have exactly one @".to_string(),
            ));
        }

        if parts[0].is_empty() || parts[1].is_empty() {
            return Err(SyncError::Validation(
                "Email local and domain parts cannot be empty".to_string(),
            ));
        }

        if !parts[1].contains('.') {
            return Err(SyncError::Validation(
                "Email domain must contain a dot".to_string(),
            ));
        }

        Ok(())
    }
}

impl std::fmt::Display for EmailAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 파일 경로 값 객체
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilePath(String);

impl FilePath {
    /// 새 파일 경로 생성
    pub fn new(path: String) -> Result<Self> {
        let path_obj = Self(path);
        path_obj.validate()?;
        Ok(path_obj)
    }

    /// 경로 문자열 반환
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 파일명 추출
    pub fn filename(&self) -> Option<&str> {
        std::path::Path::new(&self.0).file_name()?.to_str()
    }

    /// 디렉토리 경로 추출
    pub fn parent(&self) -> Option<FilePath> {
        let parent_path = std::path::Path::new(&self.0).parent()?.to_str()?;
        FilePath::new(parent_path.to_string()).ok()
    }

    /// 확장자 추출
    pub fn extension(&self) -> Option<&str> {
        std::path::Path::new(&self.0).extension()?.to_str()
    }

    /// 절대 경로 여부
    pub fn is_absolute(&self) -> bool {
        std::path::Path::new(&self.0).is_absolute()
    }

    /// 경로 정규화
    pub fn normalize(&self) -> Result<FilePath> {
        // 간단한 정규화 - 실제로는 더 복잡한 로직 필요
        let normalized = self.0.replace("//", "/");
        FilePath::new(normalized)
    }
}

impl ValueObject for FilePath {
    fn validate(&self) -> Result<()> {
        if self.0.is_empty() {
            return Err(SyncError::Validation(
                "File path cannot be empty".to_string(),
            ));
        }

        // 위험한 경로 패턴 검사
        if self.0.contains("..") {
            return Err(SyncError::Validation(
                "File path cannot contain '..'".to_string(),
            ));
        }

        // 플랫폼별 금지 문자 검사 (Windows)
        let forbidden_chars = ['<', '>', ':', '"', '|', '?', '*'];
        if forbidden_chars.iter().any(|&c| self.0.contains(c)) {
            return Err(SyncError::Validation(
                "File path contains forbidden characters".to_string(),
            ));
        }

        Ok(())
    }
}

impl std::fmt::Display for FilePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 파일 해시 값 객체
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileHash {
    algorithm: HashAlgorithm,
    value: String,
}

/// 해시 알고리즘
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HashAlgorithm {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl FileHash {
    /// 새 파일 해시 생성
    pub fn new(algorithm: HashAlgorithm, value: String) -> Result<Self> {
        let hash = Self { algorithm, value };
        hash.validate()?;
        Ok(hash)
    }

    /// SHA256 해시 생성
    pub fn sha256(value: String) -> Result<Self> {
        Self::new(HashAlgorithm::Sha256, value)
    }

    /// MD5 해시 생성
    pub fn md5(value: String) -> Result<Self> {
        Self::new(HashAlgorithm::Md5, value)
    }

    /// 해시 값 반환
    pub fn value(&self) -> &str {
        &self.value
    }

    /// 알고리즘 반환
    pub fn algorithm(&self) -> &HashAlgorithm {
        &self.algorithm
    }

    /// 예상 길이 반환
    fn expected_length(&self) -> usize {
        match self.algorithm {
            HashAlgorithm::Md5 => 32,
            HashAlgorithm::Sha1 => 40,
            HashAlgorithm::Sha256 => 64,
            HashAlgorithm::Sha512 => 128,
        }
    }
}

impl ValueObject for FileHash {
    fn validate(&self) -> Result<()> {
        if self.value.is_empty() {
            return Err(SyncError::Validation(
                "Hash value cannot be empty".to_string(),
            ));
        }

        // 길이 검증
        if self.value.len() != self.expected_length() {
            return Err(SyncError::Validation(format!(
                "Hash value length {} does not match expected length {} for {:?}",
                self.value.len(),
                self.expected_length(),
                self.algorithm
            )));
        }

        // 16진수 문자만 포함하는지 검증
        if !self.value.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(SyncError::Validation(
                "Hash value must contain only hexadecimal characters".to_string(),
            ));
        }

        Ok(())
    }
}

impl std::fmt::Display for FileHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}:{}", self.algorithm, self.value)
    }
}

/// 파일 크기 값 객체
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSize(u64);

impl FileSize {
    /// 새 파일 크기 생성
    pub fn new(bytes: u64) -> Self {
        Self(bytes)
    }

    /// 바이트 단위 크기 반환
    pub fn bytes(&self) -> u64 {
        self.0
    }

    /// KB 단위 크기 반환
    pub fn kilobytes(&self) -> f64 {
        self.0 as f64 / 1024.0
    }

    /// MB 단위 크기 반환
    pub fn megabytes(&self) -> f64 {
        self.0 as f64 / (1024.0 * 1024.0)
    }

    /// GB 단위 크기 반환
    pub fn gigabytes(&self) -> f64 {
        self.0 as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    /// 사람이 읽기 쉬운 형태로 변환
    pub fn human_readable(&self) -> String {
        if self.0 < 1024 {
            format!("{} B", self.0)
        } else if self.0 < 1024 * 1024 {
            format!("{:.1} KB", self.kilobytes())
        } else if self.0 < 1024 * 1024 * 1024 {
            format!("{:.1} MB", self.megabytes())
        } else {
            format!("{:.1} GB", self.gigabytes())
        }
    }
}

impl ValueObject for FileSize {
    fn validate(&self) -> Result<()> {
        // 파일 크기는 기본적으로 항상 유효 (0도 유효한 크기)
        Ok(())
    }
}

impl std::fmt::Display for FileSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.human_readable())
    }
}

/// 타임스탬프 값 객체
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timestamp(i64);

impl Timestamp {
    /// 현재 시간으로 생성
    pub fn now() -> Self {
        Self(chrono::Utc::now().timestamp())
    }

    /// Unix 타임스탬프로 생성
    pub fn from_unix(timestamp: i64) -> Result<Self> {
        let ts = Self(timestamp);
        ts.validate()?;
        Ok(ts)
    }

    /// Unix 타임스탬프 반환
    pub fn unix(&self) -> i64 {
        self.0
    }

    /// DateTime으로 변환
    pub fn to_datetime(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        chrono::DateTime::from_timestamp(self.0, 0)
    }

    /// ISO 8601 문자열로 변환
    pub fn to_iso8601(&self) -> String {
        self.to_datetime()
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| format!("invalid-timestamp-{}", self.0))
    }
}

impl ValueObject for Timestamp {
    fn validate(&self) -> Result<()> {
        // 1970년 1월 1일 이후의 타임스탬프만 허용
        if self.0 < 0 {
            return Err(SyncError::Validation(
                "Timestamp cannot be negative".to_string(),
            ));
        }

        // 2100년 이후의 타임스탬프는 허용하지 않음 (실용적 제한)
        if self.0 > 4102444800 {
            return Err(SyncError::Validation(
                "Timestamp is too far in the future".to_string(),
            ));
        }

        Ok(())
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_iso8601())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_address() {
        // 유효한 이메일
        let email = EmailAddress::new("test@example.com".to_string()).unwrap();
        assert_eq!(email.as_str(), "test@example.com");
        assert_eq!(email.local_part(), Some("test"));
        assert_eq!(email.domain(), Some("example.com"));

        // 무효한 이메일
        assert!(EmailAddress::new("invalid-email".to_string()).is_err());
        assert!(EmailAddress::new("@example.com".to_string()).is_err());
        assert!(EmailAddress::new("test@".to_string()).is_err());
    }

    #[test]
    fn test_file_path() {
        // 유효한 경로
        let path = FilePath::new("/home/user/file.txt".to_string()).unwrap();
        assert_eq!(path.filename(), Some("file.txt"));
        assert_eq!(path.extension(), Some("txt"));
        assert!(path.is_absolute());

        // 무효한 경로
        assert!(FilePath::new("../../../etc/passwd".to_string()).is_err());
        assert!(FilePath::new("file<name>.txt".to_string()).is_err());
    }

    #[test]
    fn test_file_hash() {
        // 유효한 SHA256 해시
        let hash = FileHash::sha256("a".repeat(64)).unwrap();
        assert_eq!(hash.value().len(), 64);
        assert_eq!(hash.algorithm(), &HashAlgorithm::Sha256);

        // 무효한 해시
        assert!(FileHash::sha256("invalid".to_string()).is_err());
        assert!(FileHash::sha256("xyz123".to_string()).is_err());
    }

    #[test]
    fn test_file_size() {
        let size = FileSize::new(1024 * 1024); // 1MB
        assert_eq!(size.bytes(), 1024 * 1024);
        assert_eq!(size.kilobytes(), 1024.0);
        assert_eq!(size.megabytes(), 1.0);
        assert_eq!(size.human_readable(), "1.0 MB");
    }

    #[test]
    fn test_timestamp() {
        let now = Timestamp::now();
        assert!(now.unix() > 0);
        assert!(now.to_datetime().is_some());

        // 유효한 타임스탬프
        let ts = Timestamp::from_unix(1640995200).unwrap(); // 2022-01-01
        assert!(ts.to_datetime().is_some());

        // 무효한 타임스탬프
        assert!(Timestamp::from_unix(-1).is_err());
        assert!(Timestamp::from_unix(5000000000).is_err());
    }
}
