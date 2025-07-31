use super::ComponentMetrics;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Generic metrics collector for any metrics type
#[derive(Clone)]
pub struct MetricsCollector<M: ComponentMetrics> {
    current_metrics: Arc<RwLock<Option<M>>>,
    historical_metrics: Arc<RwLock<Vec<M>>>,
    max_history: usize,
}

impl<M: ComponentMetrics> MetricsCollector<M> {
    pub fn new() -> Self {
        Self {
            current_metrics: Arc::new(RwLock::new(None)),
            historical_metrics: Arc::new(RwLock::new(Vec::new())),
            max_history: 100, // Keep last 100 metric snapshots
        }
    }
    
    pub fn new_with_history_size(max_history: usize) -> Self {
        Self {
            current_metrics: Arc::new(RwLock::new(None)),
            historical_metrics: Arc::new(RwLock::new(Vec::new())),
            max_history,
        }
    }
    
    pub async fn update(&self, metrics: M) {
        // Update current metrics
        {
            let mut current = self.current_metrics.write().await;
            *current = Some(metrics.clone());
        }
        
        // Add to history
        {
            let mut history = self.historical_metrics.write().await;
            history.push(metrics);
            
            // Keep only the most recent entries
            if history.len() > self.max_history {
                history.remove(0);
            }
        }
        
        debug!("📊 Metrics updated");
    }
    
    pub async fn get_current(&self) -> Option<M> {
        let current = self.current_metrics.read().await;
        current.clone()
    }
    
    pub async fn get_history(&self) -> Vec<M> {
        let history = self.historical_metrics.read().await;
        history.clone()
    }
    
    pub async fn get_aggregated(&self) -> Option<M> {
        let history = self.historical_metrics.read().await;
        if history.is_empty() {
            return None;
        }
        
        let mut aggregated = history[0].clone();
        for metrics in history.iter().skip(1) {
            aggregated.merge(metrics);
        }
        
        Some(aggregated)
    }
    
    pub async fn reset(&self) {
        {
            let mut current = self.current_metrics.write().await;
            if let Some(ref mut metrics) = *current {
                metrics.reset();
            }
        }
        
        {
            let mut history = self.historical_metrics.write().await;
            history.clear();
        }
        
        debug!("📊 Metrics reset");
    }
} 