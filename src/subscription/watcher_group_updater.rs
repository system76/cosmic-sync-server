/// Ping all subscribers for health checking
pub async fn ping_subscribers(&self) {
    let subscribers = self.get_subscribers().await;
    debug!("Pinging {} watcher group subscribers", subscribers.len());
    
    for (account_hash, device_hash) in subscribers {
        let key = format!("{}:{}", account_hash, device_hash);
        debug!("Pinging watcher group subscriber: {}", key);
        
        // 핑 메시지 생성
        let ping_notification = crate::sync::WatcherGroupUpdateNotification {
            account_hash: account_hash,
            device_hash: device_hash,
            group_data: None, // 데이터 없음 (핑 목적)
            update_type: 0, // CREATED
            timestamp: chrono::Utc::now().timestamp(),
        };
        
        // 특정 구독자에게만 핑 전송
        if let Err(e) = self.notification_manager.ping_watcher_group_subscriber(&key, ping_notification).await {
            warn!("Failed to ping watcher group subscriber {}: {}", key, e);
        }
    }
}

/// Get all active subscribers
async fn get_subscribers(&self) -> Vec<(String, String)> {
    let mut result = Vec::new();
    
    // 알림 관리자에서 구독자 목록 가져오기
    if let Ok(subscribers) = self.notification_manager.get_watcher_group_subscribers().await {
        for key in subscribers {
            // key format: "account_hash:device_hash"
            if let Some((account_hash, device_hash)) = key.split_once(':') {
                result.push((account_hash.to_string(), device_hash.to_string()));
            }
        }
    }
    
    result
} 