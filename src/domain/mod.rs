// Domain layer - Business logic and domain entities
// 기존 models 모듈을 점진적으로 도메인 엔티티로 마이그레이션

pub mod entities;
pub mod services;
pub mod repositories;
pub mod value_objects;
pub mod events;

// Re-export for convenience
pub use entities::*;
pub use services::*;
pub use repositories::*;
pub use value_objects::*;
pub use events::*;

use crate::error::Result;
use async_trait::async_trait;

/// 도메인 서비스 트레이트
/// 복잡한 비즈니스 로직을 캡슐화
#[async_trait]
pub trait DomainService: Send + Sync {
    /// 서비스 이름
    fn name(&self) -> &'static str;
    
    /// 서비스 헬스체크
    async fn health_check(&self) -> Result<bool>;
}

/// 도메인 이벤트 핸들러
#[async_trait]
pub trait DomainEventHandler<T: DomainEvent>: Send + Sync {
    async fn handle(&self, event: &T) -> Result<()>;
}

/// 도메인 이벤트 트레이트
pub trait DomainEvent: Send + Sync + Clone {
    /// 이벤트 타입
    fn event_type(&self) -> &'static str;
    
    /// 이벤트 발생 시간
    fn timestamp(&self) -> i64;
    
    /// 이벤트 ID
    fn event_id(&self) -> String;
}

/// 도메인 엔티티 기본 트레이트
pub trait Entity: Send + Sync + Clone {
    type Id: Clone + PartialEq + Send + Sync;
    
    /// 엔티티 ID
    fn id(&self) -> &Self::Id;
    
    /// 엔티티 검증
    fn validate(&self) -> Result<()>;
}

/// 애그리게이트 루트 트레이트
pub trait AggregateRoot: Entity {
    type Event: DomainEvent;
    
    /// 도메인 이벤트 가져오기
    fn events(&self) -> Vec<Self::Event>;
    
    /// 도메인 이벤트 지우기
    fn clear_events(&mut self);
}

/// 값 객체 트레이트
pub trait ValueObject: Clone + PartialEq + Send + Sync {
    /// 값 객체 검증
    fn validate(&self) -> Result<()>;
}

/// 리포지토리 패턴 기본 트레이트
#[async_trait]
pub trait Repository<T: Entity>: Send + Sync {
    /// 엔티티 저장
    async fn save(&self, entity: &T) -> Result<()>;
    
    /// ID로 엔티티 조회
    async fn find_by_id(&self, id: &T::Id) -> Result<Option<T>>;
    
    /// 엔티티 삭제
    async fn delete(&self, id: &T::Id) -> Result<()>;
    
    /// 모든 엔티티 조회
    async fn find_all(&self) -> Result<Vec<T>>;
}

/// 명세 패턴 트레이트
pub trait Specification<T> {
    /// 명세를 만족하는지 확인
    fn is_satisfied_by(&self, entity: &T) -> bool;
}

/// 팩토리 패턴 트레이트
pub trait EntityFactory<T: Entity> {
    /// 엔티티 생성
    fn create(&self) -> Result<T>;
}
