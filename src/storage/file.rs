use std::io::Error;
use crate::storage::Result;

#[async_trait]
pub trait FileStorage: Send + Sync {
    // ... existing code ...
    
    /// 파일 ID로 존재 여부와 삭제 상태 확인
    async fn check_file_exists(&self, file_id: u64) -> Result<(bool, bool)>;
    
    // ... existing code ...
} 