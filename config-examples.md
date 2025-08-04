# AWS Secrets Manager Configuration Guide

이 문서는 COSMIC Sync Server에서 AWS Secrets Manager를 사용하여 환경별 설정을 관리하는 방법을 설명합니다.

## 환경 설정

서버는 `ENV` 환경 변수를 통해 현재 환경을 감지합니다:

- `development` (기본값): 로컬 환경 변수 사용
- `staging`: AWS Secrets Manager 사용
- `production`: AWS Secrets Manager 사용

## 로컬 개발 환경 (.env 파일)

```bash
# Environment Configuration
ENV=development

# Database
DB_HOST=localhost
DB_PORT=3306
DB_NAME=cosmic_sync
DB_USER=root
DB_PASS=recognizer
DB_POOL=5
DATABASE_CONNECTION_TIMEOUT=30
DATABASE_LOG_QUERIES=false

# Server
SERVER_HOST=0.0.0.0
SERVER_PORT=50051
WORKER_THREADS=4
AUTH_TOKEN_EXPIRY_HOURS=24
MAX_FILE_SIZE=52428800
MAX_CONCURRENT_REQUESTS=100

# Storage
STORAGE_TYPE=database
STORAGE_PATH=/tmp/cosmic-sync

# S3 Configuration (if using S3 storage)
S3_BUCKET=cosmic-sync-files
S3_KEY_PREFIX=files/
S3_ENDPOINT_URL=http://localhost:9000
S3_FORCE_PATH_STYLE=true
S3_TIMEOUT_SECONDS=30
S3_MAX_RETRIES=3

# Logging
LOG_LEVEL=info
LOG_TO_FILE=true
LOG_FILE=logs/cosmic-sync-server.log
LOG_MAX_FILE_SIZE=10485760
LOG_MAX_BACKUPS=5
LOG_FORMAT=text

# Feature Flags
COSMIC_SYNC_TEST_MODE=false
COSMIC_SYNC_DEBUG_MODE=false
ENABLE_METRICS=false
STORAGE_ENCRYPTION=true
REQUEST_VALIDATION=true

# Container Mode (optional)
COSMIC_SYNC_USE_CONTAINER=false

# Rust Logging
RUST_LOG=cosmic_sync_server=info,info
```

## AWS Secrets Manager 설정 (Staging/Production)

### 1. 환경 변수 설정

```bash
ENV=staging  # or production
AWS_REGION=us-east-1
AWS_SECRET_NAME=cosmic-sync-server-config
```

### 2. AWS Secrets Manager 시크릿 생성

AWS CLI를 사용하여 시크릿을 생성합니다:

```bash
aws secretsmanager create-secret \
    --name cosmic-sync-server-config \
    --description "COSMIC Sync Server Configuration" \
    --secret-string file://secret.json \
    --region us-east-1
```

### 3. 시크릿 JSON 형식 (secret.json)

```json
{
  "DB_HOST": "your-rds-endpoint.amazonaws.com",
  "DB_PORT": "3306",
  "DB_NAME": "cosmic_sync_prod",
  "DB_USER": "cosmic_user",
  "DB_PASS": "your-secure-password",
  "DB_POOL": "10",
  "DATABASE_CONNECTION_TIMEOUT": "30",
  "DATABASE_LOG_QUERIES": "false",
  
  "SERVER_HOST": "0.0.0.0",
  "SERVER_PORT": "50051",
  "WORKER_THREADS": "8",
  "AUTH_TOKEN_EXPIRY_HOURS": "24",
  "MAX_FILE_SIZE": "104857600",
  "MAX_CONCURRENT_REQUESTS": "1000",
  
  "STORAGE_TYPE": "s3",
  "S3_BUCKET": "cosmic-sync-prod-files",
  "S3_KEY_PREFIX": "files/",
  "S3_TIMEOUT_SECONDS": "30",
  "S3_MAX_RETRIES": "3",
  
  "LOG_LEVEL": "info",
  "LOG_TO_FILE": "true",
  "LOG_FILE": "/var/log/cosmic-sync-server.log",
  "LOG_MAX_FILE_SIZE": "10485760",
  "LOG_MAX_BACKUPS": "10",
  "LOG_FORMAT": "json",
  
  "COSMIC_SYNC_TEST_MODE": "false",
  "COSMIC_SYNC_DEBUG_MODE": "false",
  "ENABLE_METRICS": "true",
  "STORAGE_ENCRYPTION": "true",
  "REQUEST_VALIDATION": "true"
}
```

### 4. AWS IAM 권한 설정

서버가 실행되는 EC2 인스턴스나 ECS 태스크에 다음 IAM 정책을 연결해야 합니다:

```json
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Effect": "Allow",
            "Action": [
                "secretsmanager:GetSecretValue"
            ],
            "Resource": "arn:aws:secretsmanager:us-east-1:YOUR-ACCOUNT-ID:secret:cosmic-sync-server-config*"
        }
    ]
}
```

## 사용 방법

### 1. 환경별 설정 로딩

서버는 시작시 자동으로 환경을 감지하고 적절한 설정을 로드합니다:

```rust
// 환경 감지 및 설정 로드 (main.rs에서 자동 처리)
let config = Config::load_async().await?;
```

### 2. 설정 우선순위

1. **환경 변수** (항상 최우선)
2. **AWS Secrets Manager** (staging/production 환경에서)
3. **기본값** (설정이 없는 경우)

### 3. 로그 확인

서버 시작시 다음과 같은 로그를 통해 설정 로딩 상태를 확인할 수 있습니다:

```
INFO cosmic_sync_server: 🔧 Detected environment: Production
INFO cosmic_sync_server: ☁️ Cloud environment detected - using AWS Secrets Manager for sensitive values
INFO cosmic_sync_server: 🌐 Server will listen on 0.0.0.0:50051
INFO cosmic_sync_server: 🗄️ Database: cosmic_user@your-rds-endpoint.amazonaws.com
```

## 환경별 배포 전략

### Development
- `.env` 파일 또는 로컬 환경 변수 사용
- 민감하지 않은 개발용 값들 설정

### Staging
- AWS Secrets Manager 사용
- Production과 유사하지만 별도의 리소스 사용
- 테스트용 데이터베이스 및 S3 버킷

### Production
- AWS Secrets Manager 사용
- 강화된 보안 설정
- 모니터링 및 로깅 활성화

## 시크릿 업데이트

AWS Secrets Manager의 시크릿을 업데이트하면 서버 재시작 없이 새로운 값이 적용됩니다:

```bash
aws secretsmanager update-secret \
    --secret-id cosmic-sync-server-config \
    --secret-string file://updated-secret.json \
    --region us-east-1
```

## 문제 해결

### 1. AWS Secrets Manager 연결 실패
- IAM 권한 확인
- AWS 리전 설정 확인
- 시크릿 이름 확인

### 2. 설정 값 누락
- 시크릿 JSON 형식 확인
- 환경 변수 대체 설정 확인

### 3. 권한 오류
- EC2 인스턴스 역할 또는 ECS 태스크 역할 확인
- IAM 정책의 리소스 ARN 확인

## 보안 고려사항

1. **시크릿 로테이션**: 정기적으로 데이터베이스 비밀번호 등을 로테이션
2. **최소 권한 원칙**: 필요한 시크릿에만 접근 권한 부여
3. **로그 마스킹**: 민감한 정보가 로그에 노출되지 않도록 주의
4. **네트워크 보안**: VPC 내부에서만 접근 가능하도록 설정 