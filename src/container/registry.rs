use crate::{
    container::AppContainer,
    error::{Result, SyncError},
};
use async_trait::async_trait;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 서비스 레지스트리
/// 타입 안전한 서비스 등록 및 조회를 제공
pub struct ServiceRegistry {
    services: Arc<RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,
    factories: Arc<RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,
}

impl ServiceRegistry {
    /// 새 레지스트리 인스턴스 생성
    pub fn new() -> Self {
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            factories: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 서비스 인스턴스 등록
    pub async fn register<T: Send + Sync + 'static>(&self, service: T) {
        let type_id = TypeId::of::<T>();
        let mut services = self.services.write().await;
        services.insert(type_id, Box::new(service));
    }

    /// 서비스 팩토리 등록
    pub async fn register_factory<T: Send + Sync + 'static>(
        &self,
        factory: Box<dyn ServiceFactory<Service = T> + Send + Sync>,
    ) {
        let type_id = TypeId::of::<T>();
        let mut factories = self.factories.write().await;
        factories.insert(type_id, Box::new(factory));
    }

    /// 서비스 조회
    pub async fn get<T: Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        let type_id = TypeId::of::<T>();

        // 먼저 등록된 인스턴스 확인
        {
            let services = self.services.read().await;
            if let Some(service) = services.get(&type_id) {
                if let Some(typed_service) = service.downcast_ref::<Arc<T>>() {
                    return Ok(typed_service.clone());
                }
                if let Some(typed_service) = service.downcast_ref::<T>() {
                    // T가 직접 저장된 경우 Arc로 감싸서 반환
                    return Err(SyncError::Internal(
                        "Service stored as value instead of Arc".to_string(),
                    ));
                }
            }
        }

        // 팩토리를 통한 서비스 생성
        {
            let factories = self.factories.read().await;
            if let Some(factory) = factories.get(&type_id) {
                if let Some(typed_factory) =
                    factory.downcast_ref::<Box<dyn ServiceFactory<Service = T> + Send + Sync>>()
                {
                    let service = typed_factory.create().await?;

                    // 생성된 서비스를 캐시
                    drop(factories);
                    self.register(service.clone()).await;

                    return Ok(service);
                }
            }
        }

        Err(SyncError::NotFound(format!(
            "Service not found for type: {}",
            std::any::type_name::<T>()
        )))
    }

    /// 서비스 존재 여부 확인
    pub async fn contains<T: Send + Sync + 'static>(&self) -> bool {
        let type_id = TypeId::of::<T>();
        let services = self.services.read().await;
        let factories = self.factories.read().await;

        services.contains_key(&type_id) || factories.contains_key(&type_id)
    }

    /// 모든 서비스 정리
    pub async fn clear(&self) {
        let mut services = self.services.write().await;
        let mut factories = self.factories.write().await;

        services.clear();
        factories.clear();
    }

    /// 등록된 서비스 개수
    pub async fn service_count(&self) -> usize {
        let services = self.services.read().await;
        services.len()
    }

    /// 등록된 팩토리 개수
    pub async fn factory_count(&self) -> usize {
        let factories = self.factories.read().await;
        factories.len()
    }
}

impl Default for ServiceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 서비스 팩토리 트레이트
#[async_trait]
pub trait ServiceFactory {
    type Service: Send + Sync + 'static;

    /// 서비스 인스턴스 생성
    async fn create(&self) -> Result<Arc<Self::Service>>;

    /// Any 트레이트로 다운캐스팅을 위한 메서드
    fn as_any(&self) -> &dyn Any;
}

/// 지연 초기화 팩토리
pub struct LazyFactory<T, F>
where
    T: Send + Sync + 'static,
    F: Fn() -> Result<Arc<T>> + Send + Sync + 'static,
{
    factory_fn: F,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, F> LazyFactory<T, F>
where
    T: Send + Sync + 'static,
    F: Fn() -> Result<Arc<T>> + Send + Sync + 'static,
{
    pub fn new(factory_fn: F) -> Self {
        Self {
            factory_fn,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<T, F> ServiceFactory for LazyFactory<T, F>
where
    T: Send + Sync + 'static,
    F: Fn() -> Result<Arc<T>> + Send + Sync + 'static,
{
    type Service = T;

    async fn create(&self) -> Result<Arc<Self::Service>> {
        (self.factory_fn)()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 비동기 지연 초기화 팩토리
pub struct AsyncLazyFactory<T, F, Fut>
where
    T: Send + Sync + 'static,
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Arc<T>>> + Send + Sync + 'static,
{
    factory_fn: F,
    _phantom: std::marker::PhantomData<(T, Fut)>,
}

impl<T, F, Fut> AsyncLazyFactory<T, F, Fut>
where
    T: Send + Sync + 'static,
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Arc<T>>> + Send + Sync + 'static,
{
    pub fn new(factory_fn: F) -> Self {
        Self {
            factory_fn,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<T, F, Fut> ServiceFactory for AsyncLazyFactory<T, F, Fut>
where
    T: Send + Sync + 'static,
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Arc<T>>> + Send + Sync + 'static,
{
    type Service = T;

    async fn create(&self) -> Result<Arc<Self::Service>> {
        (self.factory_fn)().await
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 편의를 위한 매크로들
/// 서비스 등록을 단순화
#[macro_export]
macro_rules! register_service {
    ($registry:expr, $service:expr) => {
        $registry.register($service).await
    };
}

#[macro_export]
macro_rules! register_factory {
    ($registry:expr, $factory:expr) => {
        $registry.register_factory(Box::new($factory)).await
    };
}

#[macro_export]
macro_rules! get_service {
    ($registry:expr, $service_type:ty) => {
        $registry.get::<$service_type>().await
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestService {
        id: usize,
    }

    impl TestService {
        fn new(id: usize) -> Self {
            Self { id }
        }
    }

    #[tokio::test]
    async fn test_service_registry_basic() {
        let registry = ServiceRegistry::new();
        let service = Arc::new(TestService::new(1));

        // 서비스 등록
        registry.register(service.clone()).await;

        // 서비스 조회
        let retrieved = registry.get::<TestService>().await.unwrap();
        assert_eq!(retrieved.id, 1);

        // 존재 확인
        assert!(registry.contains::<TestService>().await);

        // 개수 확인
        assert_eq!(registry.service_count().await, 1);
    }

    #[tokio::test]
    async fn test_lazy_factory() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);

        let registry = ServiceRegistry::new();

        // 지연 팩토리 등록
        let factory = LazyFactory::new(|| {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(TestService::new(id)))
        });

        registry
            .register_factory::<TestService>(Box::new(factory))
            .await;

        // 첫 번째 조회 - 팩토리를 통해 생성
        let service1 = registry.get::<TestService>().await.unwrap();
        assert_eq!(service1.id, 0);

        // 두 번째 조회 - 캐시된 인스턴스 반환
        let service2 = registry.get::<TestService>().await.unwrap();
        assert_eq!(service2.id, 0); // 같은 인스턴스

        // 팩토리는 한 번만 호출되어야 함
        assert_eq!(COUNTER.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_async_lazy_factory() {
        let registry = ServiceRegistry::new();

        // 비동기 지연 팩토리 등록
        let factory = AsyncLazyFactory::new(|| async {
            // 비동기 작업 시뮬레이션
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            Ok(Arc::new(TestService::new(42)))
        });

        registry
            .register_factory::<TestService>(Box::new(factory))
            .await;

        // 서비스 조회
        let service = registry.get::<TestService>().await.unwrap();
        assert_eq!(service.id, 42);
    }

    #[tokio::test]
    async fn test_registry_clear() {
        let registry = ServiceRegistry::new();

        // 서비스와 팩토리 등록
        registry.register(Arc::new(TestService::new(1))).await;

        let factory = LazyFactory::new(|| Ok(Arc::new(TestService::new(2))));
        registry
            .register_factory::<TestService>(Box::new(factory))
            .await;

        assert_eq!(registry.service_count().await, 1);
        assert_eq!(registry.factory_count().await, 1);

        // 모든 것 정리
        registry.clear().await;

        assert_eq!(registry.service_count().await, 0);
        assert_eq!(registry.factory_count().await, 0);
    }
}
