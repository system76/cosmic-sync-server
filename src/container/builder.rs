use crate::{
    config::settings::Config,
    container::AppContainer,
    error::Result,
    services::{AuthService, DeviceService, EncryptionService, FileService},
    storage::{init_storage, Storage},
};
use std::sync::Arc;
use tracing::{info, instrument};

/// 의존성 주입 컨테이너 빌더
/// 기존 초기화 로직을 유지하면서 점진적으로 개선
pub struct ContainerBuilder {
    config: Option<Arc<Config>>,
    storage: Option<Arc<dyn Storage>>,
}

impl ContainerBuilder {
    /// 새 빌더 인스턴스 생성
    pub fn new() -> Self {
        Self {
            config: None,
            storage: None,
        }
    }

    /// 설정을 빌더에 추가
    pub fn with_config(mut self, config: Config) -> Self {
        self.config = Some(Arc::new(config));
        self
    }

    /// 외부에서 생성된 스토리지를 사용 (테스트용)
    pub fn with_storage(mut self, storage: Arc<dyn Storage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// 컨테이너 빌드
    #[instrument(skip(self))]
    pub async fn build(self) -> Result<AppContainer> {
        info!("🔧 Building application container");

        // 설정 로드 (필수)
        let config = self.config.clone().unwrap_or_else(|| {
            info!("📋 Loading default configuration");
            Arc::new(Config::load())
        });

        // 스토리지 초기화
        let storage = if let Some(ref storage) = self.storage {
            info!("📊 Using provided storage instance");
            storage.clone()
        } else {
            if cfg!(test) || config.features.test_mode {
                info!("📊 Using in-memory storage (test mode)");
                Arc::new(crate::storage::memory::MemoryStorage::new()) as Arc<dyn Storage>
            } else {
                info!("📊 Initializing storage from configuration");
                init_storage(&config.database).await?
            }
        };

        // 서비스들 초기화 (기존 로직 유지)
        let services = self.build_services(&storage, &config).await?;

        let container = AppContainer::new(
            storage,
            services.auth_service,
            services.device_service,
            services.file_service,
            services.encryption_service,
            config,
        );

        info!("✅ Application container built successfully");
        Ok(container)
    }

    /// 서비스들을 초기화하는 내부 메서드
    #[instrument(skip(self, storage, config))]
    async fn build_services(
        &self,
        storage: &Arc<dyn Storage>,
        config: &Arc<Config>,
    ) -> Result<Services> {
        info!("🔧 Initializing application services");

        // 각 서비스를 의존성과 함께 초기화
        let auth_service = Arc::new(AuthService::new(storage.clone()));
        let device_service = Arc::new(DeviceService::with_storage(storage.clone()));
        let file_service = Arc::new(FileService::with_storage(storage.clone()));
        let encryption_service = Arc::new(EncryptionService::new(storage.clone()));

        info!("✅ All services initialized successfully");

        Ok(Services {
            auth_service,
            device_service,
            file_service,
            encryption_service,
        })
    }
}

impl Default for ContainerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 서비스 컬렉션 (내부 사용)
struct Services {
    auth_service: Arc<AuthService>,
    device_service: Arc<DeviceService>,
    file_service: Arc<FileService>,
    encryption_service: Arc<EncryptionService>,
}

/// 편의를 위한 빌더 팩토리 메서드들
impl ContainerBuilder {
    /// 개발 환경용 컨테이너 (메모리 스토리지)
    #[cfg(debug_assertions)]
    pub async fn build_dev() -> Result<AppContainer> {
        info!("🔧 Building development container with memory storage");

        let config = Config::load();
        let storage = Arc::new(crate::storage::memory::MemoryStorage::new());

        Self::new()
            .with_config(config)
            .with_storage(storage)
            .build()
            .await
    }

    /// 프로덕션 환경용 컨테이너
    pub async fn build_production() -> Result<AppContainer> {
        info!("🔧 Building production container");

        let config = Config::load();

        Self::new().with_config(config).build().await
    }

    /// 테스트용 컨테이너
    #[cfg(test)]
    pub async fn build_test() -> Result<AppContainer> {
        let config = Config {
            server: crate::config::settings::ServerConfig::default(),
            database: crate::config::settings::DatabaseConfig::default(),
            storage: crate::config::settings::StorageConfig::default(),
            logging: crate::config::settings::LoggingConfig::default(),
            features: crate::config::settings::FeatureFlags {
                test_mode: true,
                debug_mode: true,
                ..Default::default()
            },
            message_broker: crate::config::settings::MessageBrokerConfig::default(),
            server_encode_key: None,
        };

        let storage = Arc::new(crate::storage::memory::MemoryStorage::new());

        Self::new()
            .with_config(config)
            .with_storage(storage)
            .build()
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_container_builder() {
        let container = ContainerBuilder::build_test().await.unwrap();

        // 서비스들이 올바르게 초기화되었는지 확인
        assert!(container.health_check().await.unwrap());

        // 서비스 접근 테스트
        let _auth_service = container.auth_service();
        let _device_service = container.device_service();
        let _file_service = container.file_service();
        // watcher_service is not available; ensure other services are accessible.
        let _encryption_service = container.encryption_service();
        let _config = container.config();

        // 정리
        container.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_builder_with_custom_config() {
        let mut config = Config::default();
        config.features.test_mode = true;

        let container = ContainerBuilder::new()
            .with_config(config)
            .build()
            .await
            .unwrap();

        assert!(container.config().features.test_mode);

        container.shutdown().await.unwrap();
    }
}
