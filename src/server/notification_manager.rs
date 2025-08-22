use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tonic::Status;
use tracing::{debug, error, info, warn};
use crate::sync::{FileUpdateNotification, WatcherGroupUpdateNotification, WatcherPresetUpdateNotification};
use thiserror::Error;

/// 알림 관리자 오류 타입
#[derive(Error, Debug)]
pub enum NotificationError {
    #[error("Failed to send notification: {0}")]
    SendError(String),
    
    #[error("Subscriber not found: {0}")]
    NotFound(String),
    
    #[error("Internal error: {0}")]
    Internal(String),
}

/// 알림 관리자 결과 타입
pub type Result<T> = std::result::Result<T, NotificationError>;

/// 파일 업데이트 알림을 위한 송신자 타입
pub type FileUpdateSender = mpsc::Sender<std::result::Result<FileUpdateNotification, Status>>;

/// 워처 그룹 업데이트 알림을 위한 송신자 타입
pub type WatcherGroupUpdateSender = mpsc::Sender<std::result::Result<WatcherGroupUpdateNotification, Status>>;

/// 워처 프리셋 업데이트 알림을 위한 송신자 타입
pub type WatcherPresetUpdateSender = mpsc::Sender<std::result::Result<WatcherPresetUpdateNotification, Status>>;

/// 서비스 전반에 걸쳐 이벤트 알림 관리를 담당하는 매니저
#[derive(Clone)]
pub struct NotificationManager {
    // account_hash:device_hash -> sender 매핑
    file_update_subscribers: Arc<Mutex<HashMap<String, FileUpdateSender>>>,
    // 구독자 키 -> 클라이언트가 보낸 원본 account_hash(필터 호환용)
    file_update_alias_accounts: Arc<Mutex<HashMap<String, String>>>,
    // account_hash:device_hash -> sender 매핑
    watcher_group_update_subscribers: Arc<Mutex<HashMap<String, WatcherGroupUpdateSender>>>,
    // account_hash:device_hash -> sender 매핑
    watcher_preset_update_subscribers: Arc<Mutex<HashMap<String, WatcherPresetUpdateSender>>>,
}

impl NotificationManager {
    /// 새 NotificationManager 인스턴스 생성
    pub fn new() -> Self {
        Self {
            file_update_subscribers: Arc::new(Mutex::new(HashMap::new())),
            file_update_alias_accounts: Arc::new(Mutex::new(HashMap::new())),
            watcher_group_update_subscribers: Arc::new(Mutex::new(HashMap::new())),
            watcher_preset_update_subscribers: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// 파일 업데이트 구독자 등록
    pub async fn register_file_update_subscriber(&self, key: String, sender: FileUpdateSender) -> Result<()> {
        let mut subscribers = self.file_update_subscribers.lock().await;
        subscribers.insert(key.clone(), sender);
        Ok(())
    }

    /// 파일 업데이트 구독자 등록(클라이언트 원본 account_hash 별칭 포함)
    pub async fn register_file_update_subscriber_with_alias(&self, key: String, sender: FileUpdateSender, original_account_hash: String) -> Result<()> {
        let mut subscribers = self.file_update_subscribers.lock().await;
        subscribers.insert(key.clone(), sender);
        let mut aliases = self.file_update_alias_accounts.lock().await;
        aliases.insert(key, original_account_hash);
        Ok(())
    }
    
    /// 파일 업데이트 구독자 제거
    pub async fn unregister_file_update_subscriber(&self, key: &str) -> Result<bool> {
        let mut subscribers = self.file_update_subscribers.lock().await;
        let removed = subscribers.remove(key).is_some();
        let mut aliases = self.file_update_alias_accounts.lock().await;
        aliases.remove(key);
        Ok(removed)
    }
    
    /// 워처 그룹 업데이트 구독자 등록
    pub async fn register_watcher_group_update_subscriber(&self, key: String, sender: WatcherGroupUpdateSender) -> Result<()> {
        let mut subscribers = self.watcher_group_update_subscribers.lock().await;
        subscribers.insert(key.clone(), sender);
        Ok(())
    }
    
    /// 워처 그룹 업데이트 구독자 제거
    pub async fn unregister_watcher_group_update_subscriber(&self, key: &str) -> Result<bool> {
        let mut subscribers = self.watcher_group_update_subscribers.lock().await;
        Ok(subscribers.remove(key).is_some())
    }
    
    /// 워처 프리셋 업데이트 구독자 등록
    pub async fn register_watcher_preset_update_subscriber(&self, key: String, sender: WatcherPresetUpdateSender) -> Result<()> {
        let mut subscribers = self.watcher_preset_update_subscribers.lock().await;
        subscribers.insert(key.clone(), sender);
        Ok(())
    }
    
    /// 워처 프리셋 업데이트 구독자 제거
    pub async fn unregister_watcher_preset_update_subscriber(&self, key: &str) -> Result<bool> {
        let mut subscribers = self.watcher_preset_update_subscribers.lock().await;
        Ok(subscribers.remove(key).is_some())
    }
    
    /// 파일 업데이트 알림 전송 (동일 계정의 다른 장치들에게만)
    pub async fn broadcast_file_update(&self, notification: FileUpdateNotification) -> Result<usize> {
        self.broadcast_file_update_internal(notification, true).await
    }
    
    /// 파일 업데이트 알림 전송 (동일 계정의 모든 장치들에게, 소스 장치 포함)
    /// 파일 복원 등에서 복원을 요청한 장치도 업데이트를 받아야 하는 경우 사용
    pub async fn broadcast_file_update_including_source(&self, notification: FileUpdateNotification) -> Result<usize> {
        self.broadcast_file_update_internal(notification, false).await
    }
    
    /// 내부 구현 메서드
    async fn broadcast_file_update_internal(&self, notification: FileUpdateNotification, exclude_source: bool) -> Result<usize> {
        let source_device = notification.device_hash.clone();
        let account_hash = notification.account_hash.clone();
        
        let subscribers = self.file_update_subscribers.lock().await;
        let aliases = self.file_update_alias_accounts.lock().await;
        let mut sent_count = 0;
        let mut errors = Vec::new();
        
        // 동일 계정의 모든 구독자들을 찾고 필터링
        for (device_key, sender) in subscribers.iter() {
            // 같은 account_hash를 가진 디바이스들 중에서
            if device_key.starts_with(&format!("{}:", account_hash)) {
                // exclude_source가 true이면 업로드한 장치 자신은 제외
                let should_send = if exclude_source {
                    !device_key.ends_with(&format!(":{}", source_device))
                } else {
                    true // 모든 장치에게 전송
                };
                
                if should_send {
                    // 구독자별 계정 별칭이 있다면 알림의 account_hash를 별칭으로 교체
                    let mut notif = notification.clone();
                    if let Some(alias_acc) = aliases.get(device_key) {
                        notif.account_hash = alias_acc.clone();
                    }
                    match sender.send(Ok(notif)).await {
                        Ok(_) => {
                            sent_count += 1;
                        },
                        Err(e) => {
                            let error_msg = format!("Failed to send notification to {}: {}", device_key, e);
                            error!("{}", error_msg);
                            errors.push(error_msg);
                        }
                    }
                }
            }
        }
        
        if !errors.is_empty() && sent_count == 0 {
            // Failed to send all notifications
            Err(NotificationError::SendError(errors.join("; ")))
        } else {
            // Some notifications failed, or all notifications were sent successfully
            if !errors.is_empty() {
                warn!("Some notification sends failed: {}", errors.join("; "));
            }
            Ok(sent_count)
        }
    }
    
    /// 워처 그룹 업데이트 알림 전송 (동일 계정의 다른 장치들에게만)
    pub async fn broadcast_watcher_group_update(&self, 
                                               account_hash: &str, 
                                               exclude_device_hash: Option<&str>,
                                               update: WatcherGroupUpdateNotification) -> Result<()> {
        let subscribers = self.watcher_group_update_subscribers.lock().await;
        let mut successful_sends = 0;
        let mut failed_sends = 0;
        
        for (subscriber_key, sender) in subscribers.iter() {
            // subscriber_key : "account_hash:device_hash"
            let key_parts: Vec<&str> = subscriber_key.split(':').collect();
            if key_parts.len() != 2 {
                warn!("잘못된 구독자 키 형식: {}", subscriber_key);
                continue;
            }
            
            let subscriber_account_hash = key_parts[0];
            let subscriber_device_hash = key_parts[1];
            
            // 동일 계정인지 확인
            if subscriber_account_hash != account_hash {
                continue;
            }
            
            // exclude_device_hash가 지정된 경우 해당 장치는 제외
            if let Some(exclude_hash) = exclude_device_hash {
                if subscriber_device_hash == exclude_hash {
                    debug!("제외 장치 건너뛰기: {}", subscriber_device_hash);
                    continue;
                }
            }
            
            // 알림 전송 시도
            match sender.send(Ok(update.clone())).await {
                        Ok(_) => {
                    successful_sends += 1;
                    debug!("워처 그룹 업데이트 알림 전송 성공: {} -> {}", account_hash, subscriber_device_hash);
                        },
                        Err(e) => {
                    failed_sends += 1;
                    error!("워처 그룹 업데이트 알림 전송 실패: {} -> {}: {}", account_hash, subscriber_device_hash, e);
                }
            }
        }
        
        info!("워처 그룹 업데이트 알림 전송 완료: 성공 {}, 실패 {}", successful_sends, failed_sends);
        Ok(())
    }
    
    /// 워처 그룹 구독자가 활성 상태인지 확인
    pub async fn is_watcher_group_subscriber_active(&self, key: &str) -> bool {
        let subscribers = self.watcher_group_update_subscribers.lock().await;
        subscribers.contains_key(key)
    }
    
    /// 특정 워처 그룹 구독자에게 핑 메시지 전송 (연결 확인용)
    pub async fn ping_watcher_group_subscriber(&self, key: &str, notification: WatcherGroupUpdateNotification) -> Result<()> {
        let subscribers = self.watcher_group_update_subscribers.lock().await;
        
        if let Some(sender) = subscribers.get(key) {
            match sender.send(Ok(notification)).await {
                Ok(_) => Ok(()),
                Err(e) => {
                    let error_msg = format!("Failed to send ping to {}: {}", key, e);
                    error!("{}", error_msg);
                    Err(NotificationError::SendError(error_msg))
                }
            }
        } else {
            Err(NotificationError::NotFound(format!("Subscriber not found: {}", key)))
        }
    }
    
    /// 모든 활성화된 워처 그룹 구독자 목록 가져오기
    pub async fn get_watcher_group_subscribers(&self) -> Result<Vec<String>> {
        let subscribers = self.watcher_group_update_subscribers.lock().await;
        let keys: Vec<String> = subscribers.keys().cloned().collect();
        Ok(keys)
    }

    /// 워처 프리셋 업데이트 알림 전송 (동일 계정의 다른 장치들에게만)
    pub async fn broadcast_watcher_preset_update(&self, 
                                                account_hash: &str, 
                                                exclude_device_hash: Option<&str>,
                                                update: WatcherPresetUpdateNotification) -> Result<()> {
        let subscribers = self.watcher_preset_update_subscribers.lock().await;
        let mut successful_sends = 0;
        let mut failed_sends = 0;
        
        for (subscriber_key, sender) in subscribers.iter() {
            // subscriber_key 형식: "account_hash:device_hash"
            let key_parts: Vec<&str> = subscriber_key.split(':').collect();
            if key_parts.len() != 2 {
                warn!("잘못된 구독자 키 형식: {}", subscriber_key);
                continue;
            }
            
            let subscriber_account_hash = key_parts[0];
            let subscriber_device_hash = key_parts[1];
            
            // 동일 계정인지 확인
            if subscriber_account_hash != account_hash {
                continue;
            }
            
            // exclude_device_hash가 지정된 경우 해당 장치는 제외
            if let Some(exclude_hash) = exclude_device_hash {
                if subscriber_device_hash == exclude_hash {
                    debug!("제외 장치 건너뛰기: {}", subscriber_device_hash);
                    continue;
                }
            }
            
            // 알림 전송 시도
            match sender.send(Ok(update.clone())).await {
                Ok(_) => {
                    successful_sends += 1;
                    debug!("워처 프리셋 업데이트 알림 전송 성공: {} -> {}", account_hash, subscriber_device_hash);
                },
                Err(e) => {
                    failed_sends += 1;
                    error!("워처 프리셋 업데이트 알림 전송 실패: {} -> {}: {}", account_hash, subscriber_device_hash, e);
                }
            }
        }
        
        info!("워처 프리셋 업데이트 알림 전송 완료: 성공 {}, 실패 {}", successful_sends, failed_sends);
        Ok(())
    }
    
    /// Get file update subscribers (for internal use)
    pub fn get_file_update_subscribers(&self) -> &Arc<Mutex<HashMap<String, FileUpdateSender>>> {
        &self.file_update_subscribers
    }

    /// Get alias account_hash for a given file update subscriber key, if any
    pub async fn get_file_update_alias_account(&self, key: &str) -> Option<String> {
        let aliases = self.file_update_alias_accounts.lock().await;
        aliases.get(key).cloned()
    }
} 