//! Background connection cleanup scheduler

use crate::server::connection_tracker::ConnectionTracker;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info};

/// Connection cleanup scheduler for removing old inactive connections
pub struct ConnectionCleanupScheduler {
    connection_tracker: Arc<ConnectionTracker>,
    cleanup_interval_hours: u64,
    max_age_hours: i64,
}

impl ConnectionCleanupScheduler {
    /// Create a new cleanup scheduler
    pub fn new(
        connection_tracker: Arc<ConnectionTracker>,
        cleanup_interval_hours: u64,
        max_age_hours: i64,
    ) -> Self {
        Self {
            connection_tracker,
            cleanup_interval_hours,
            max_age_hours,
        }
    }

    /// Start the background cleanup task
    pub async fn start(&self) -> tokio::task::JoinHandle<()> {
        let connection_tracker = self.connection_tracker.clone();
        let cleanup_interval_hours = self.cleanup_interval_hours;
        let max_age_hours = self.max_age_hours;

        info!(
            "🧹 Starting connection cleanup scheduler (interval: {}h, max_age: {}h)",
            cleanup_interval_hours, max_age_hours
        );

        tokio::spawn(async move {
            let mut cleanup_interval = interval(Duration::from_secs(cleanup_interval_hours * 3600));

            loop {
                cleanup_interval.tick().await;

                debug!("🧹 Running connection cleanup task");

                // Get stats before cleanup
                let stats_before = connection_tracker.get_stats().await;

                // Perform cleanup
                connection_tracker
                    .cleanup_old_connections(max_age_hours)
                    .await;

                // Get stats after cleanup
                let stats_after = connection_tracker.get_stats().await;

                if stats_before.total_connections != stats_after.total_connections {
                    info!(
                        "🧹 Connection cleanup completed: {} → {} total connections",
                        stats_before.total_connections, stats_after.total_connections
                    );
                } else {
                    debug!("🧹 Connection cleanup completed: no changes needed");
                }
            }
        })
    }
}

/// Create default cleanup scheduler
pub fn create_default_cleanup_scheduler(
    connection_tracker: Arc<ConnectionTracker>,
) -> ConnectionCleanupScheduler {
    ConnectionCleanupScheduler::new(
        connection_tracker,
        6,  // Cleanup every 6 hours
        24, // Remove connections older than 24 hours
    )
}

/// Connection statistics reporter for monitoring
pub struct ConnectionStatsReporter {
    connection_tracker: Arc<ConnectionTracker>,
    report_interval_minutes: u64,
}

impl ConnectionStatsReporter {
    /// Create a new stats reporter
    pub fn new(connection_tracker: Arc<ConnectionTracker>, report_interval_minutes: u64) -> Self {
        Self {
            connection_tracker,
            report_interval_minutes,
        }
    }

    /// Start the background stats reporting task
    pub async fn start(&self) -> tokio::task::JoinHandle<()> {
        let connection_tracker = self.connection_tracker.clone();
        let report_interval_minutes = self.report_interval_minutes;

        info!(
            "📊 Starting connection stats reporter (interval: {}min)",
            report_interval_minutes
        );

        tokio::spawn(async move {
            let mut report_interval = interval(Duration::from_secs(report_interval_minutes * 60));

            loop {
                report_interval.tick().await;

                let stats = connection_tracker.get_stats().await;

                if stats.total_connections > 0 {
                    info!(
                        "📊 Connection stats: {} total ({} active, {} inactive)",
                        stats.total_connections,
                        stats.active_connections,
                        stats.inactive_connections
                    );
                } else {
                    debug!("📊 Connection stats: no connections tracked");
                }
            }
        })
    }
}

/// Create default stats reporter
pub fn create_default_stats_reporter(
    connection_tracker: Arc<ConnectionTracker>,
) -> ConnectionStatsReporter {
    ConnectionStatsReporter::new(
        connection_tracker,
        30, // Report every 30 minutes
    )
}
