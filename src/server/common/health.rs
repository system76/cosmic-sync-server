use super::ComponentHealth;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};
use chrono::{DateTime, Utc};

/// Generic health monitor
#[derive(Clone)]
pub struct HealthMonitor {
    current_health: Arc<RwLock<ComponentHealth>>,
    health_history: Arc<RwLock<Vec<ComponentHealth>>>,
    max_history: usize,
    unhealthy_threshold: usize,
}

impl HealthMonitor {
    pub fn new() -> Self {
        Self {
            current_health: Arc::new(RwLock::new(ComponentHealth::default())),
            health_history: Arc::new(RwLock::new(Vec::new())),
            max_history: 50, // Keep last 50 health checks
            unhealthy_threshold: 3, // Consider unhealthy after 3 consecutive failures
        }
    }
    
    pub fn new_with_settings(max_history: usize, unhealthy_threshold: usize) -> Self {
        Self {
            current_health: Arc::new(RwLock::new(ComponentHealth::default())),
            health_history: Arc::new(RwLock::new(Vec::new())),
            max_history,
            unhealthy_threshold,
        }
    }
    
    pub async fn update(&self, health: ComponentHealth) {
        let is_healthy = health.is_healthy;
        
        // Update current health
        {
            let mut current = self.current_health.write().await;
            *current = health.clone();
        }
        
        // Add to history
        {
            let mut history = self.health_history.write().await;
            history.push(health);
            
            // Keep only the most recent entries
            if history.len() > self.max_history {
                history.remove(0);
            }
        }
        
        // Check for consecutive failures
        if !is_healthy {
            let consecutive_failures = self.count_recent_failures().await;
            if consecutive_failures >= self.unhealthy_threshold {
                warn!("🚨 Component has been unhealthy for {} consecutive checks", consecutive_failures);
            }
        }
        
        debug!("🏥 Health status updated: {}", if is_healthy { "healthy" } else { "unhealthy" });
    }
    
    pub async fn get_current(&self) -> ComponentHealth {
        let current = self.current_health.read().await;
        current.clone()
    }
    
    pub async fn get_history(&self) -> Vec<ComponentHealth> {
        let history = self.health_history.read().await;
        history.clone()
    }
    
    pub async fn is_consistently_healthy(&self, duration_minutes: u32) -> bool {
        let history = self.health_history.read().await;
        let threshold_time = Utc::now() - chrono::Duration::minutes(duration_minutes as i64);
        
        history
            .iter()
            .filter(|h| h.last_check >= threshold_time)
            .all(|h| h.is_healthy)
    }
    
    pub async fn count_recent_failures(&self) -> usize {
        let history = self.health_history.read().await;
        let mut failures = 0;
        
        // Count consecutive failures from the end
        for health in history.iter().rev() {
            if !health.is_healthy {
                failures += 1;
            } else {
                break;
            }
        }
        
        failures
    }
    
    pub async fn get_uptime_percentage(&self, duration_hours: u32) -> f32 {
        let history = self.health_history.read().await;
        let threshold_time = Utc::now() - chrono::Duration::hours(duration_hours as i64);
        
        let recent_checks: Vec<_> = history
            .iter()
            .filter(|h| h.last_check >= threshold_time)
            .collect();
        
        if recent_checks.is_empty() {
            return 0.0;
        }
        
        let healthy_count = recent_checks.iter().filter(|h| h.is_healthy).count();
        (healthy_count as f32 / recent_checks.len() as f32) * 100.0
    }
    
    pub async fn reset(&self) {
        {
            let mut current = self.current_health.write().await;
            *current = ComponentHealth::default();
        }
        
        {
            let mut history = self.health_history.write().await;
            history.clear();
        }
        
        debug!("🏥 Health monitor reset");
    }
} 