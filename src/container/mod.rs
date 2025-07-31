use std::sync::Arc;
use async_trait::async_trait;
use crate::{
    storage::Storage,
    services::{AuthService, DeviceService, FileService, WatcherService, EncryptionService},
    config::settings::Config,
    error::Result,
};

pub mod builder;
pub mod registry;

pub use builder::ContainerBuilder;
pub use registry::ServiceRegistry;

/// 의존성 주입 컨테이너
/// 기존 코드와의 호환성을 유지하면서 점진적으로 도입
#[derive(Clone)]
pub struct AppContainer {
    storage: Arc<dyn Storage>,
    auth_service: Arc<AuthService>,
    device_service: Arc<DeviceService>,
    file_service: Arc<FileService>,
    watcher_service: Arc<WatcherService>,
    encryption_service: Arc<EncryptionService>,
    config: Arc<Config>,
}

impl AppContainer {
    /// 컨테이너 빌더 시작
    pub fn builder() -> ContainerBuilder {
        ContainerBuilder::new()
    }

    /// 스토리지 인스턴스 가져오기
    pub fn storage(&self) -> Arc<dyn Storage> {
        self.storage.clone()
    }

    /// 인증 서비스 가져오기
    pub fn auth_service(&self) -> Arc<AuthService> {
        self.auth_service.clone()
    }

    /// 디바이스 서비스 가져오기
    pub fn device_service(&self) -> Arc<DeviceService> {
        self.device_service.clone()
    }

    /// 파일 서비스 가져오기
    pub fn file_service(&self) -> Arc<FileService> {
        self.file_service.clone()
    }

    /// 워처 서비스 가져오기
    pub fn watcher_service(&self) -> Arc<WatcherService> {
        self.watcher_service.clone()
    }

    /// 암호화 서비스 가져오기
    pub fn encryption_service(&self) -> Arc<EncryptionService> {
        self.encryption_service.clone()
    }

    /// 설정 가져오기
    pub fn config(&self) -> Arc<Config> {
        self.config.clone()
    }

    /// 헬스체크 수행
    pub async fn health_check(&self) -> Result<bool> {
        // 모든 서비스의 헬스체크 수행
        let storage_ok = self.storage.health_check().await.unwrap_or(false);
        
        // 추가 서비스 헬스체크를 여기에 추가할 수 있음
        
        Ok(storage_ok)
    }

    /// 컨테이너 종료 시 리소스 정리
    pub async fn shutdown(&self) -> Result<()> {
        // 스토리지 연결 정리
        self.storage.close().await?;
        
        // 기타 리소스 정리 작업
        
        Ok(())
    }
}

// 내부 생성자 (빌더에서만 사용)
impl AppContainer {
    pub(crate) fn new(
        storage: Arc<dyn Storage>,
        auth_service: Arc<AuthService>,
        device_service: Arc<DeviceService>,
        file_service: Arc<FileService>,
        watcher_service: Arc<WatcherService>,
        encryption_service: Arc<EncryptionService>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            storage,
            auth_service,
            device_service,
            file_service,
            watcher_service,
            encryption_service,
            config,
        }
    }
}

/// 서비스 레지스트리 트레이트
/// 향후 더 복잡한 의존성 관리가 필요할 때 확장 가능
#[async_trait]
pub trait ServiceProvider: Send + Sync {
    type Service: Send + Sync + 'static + ?Sized;
    
    async fn provide(&self, container: &AppContainer) -> Result<Arc<Self::Service>>;
}

/// 기본 서비스 제공자들
pub struct StorageProvider;
pub struct AuthServiceProvider;
pub struct DeviceServiceProvider;
pub struct FileServiceProvider;
pub struct WatcherServiceProvider;
pub struct EncryptionServiceProvider;

#[async_trait]
impl ServiceProvider for StorageProvider {
    type Service = dyn Storage;
    
    async fn provide(&self, container: &AppContainer) -> Result<Arc<Self::Service>> {
        Ok(container.storage())
    }
}

#[async_trait]
impl ServiceProvider for AuthServiceProvider {
    type Service = AuthService;
    
    async fn provide(&self, container: &AppContainer) -> Result<Arc<Self::Service>> {
        Ok(container.auth_service())
    }
}

#[async_trait]
impl ServiceProvider for DeviceServiceProvider {
    type Service = DeviceService;
    
    async fn provide(&self, container: &AppContainer) -> Result<Arc<Self::Service>> {
        Ok(container.device_service())
    }
}

#[async_trait]
impl ServiceProvider for FileServiceProvider {
    type Service = FileService;
    
    async fn provide(&self, container: &AppContainer) -> Result<Arc<Self::Service>> {
        Ok(container.file_service())
    }
}

#[async_trait]
impl ServiceProvider for WatcherServiceProvider {
    type Service = WatcherService;
    
    async fn provide(&self, container: &AppContainer) -> Result<Arc<Self::Service>> {
        Ok(container.watcher_service())
    }
}

#[async_trait]
impl ServiceProvider for EncryptionServiceProvider {
    type Service = EncryptionService;
    
    async fn provide(&self, container: &AppContainer) -> Result<Arc<Self::Service>> {
        Ok(container.encryption_service())
    }
}
