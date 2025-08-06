# 환경별 설정 및 실행 가이드

이 문서는 Cosmic Sync Server의 환경별 설정 및 실행 방법을 설명합니다.

## 🔧 지원 환경

- **Development**: 로컬 개발 환경 (`.env` 파일 사용)
- **Staging**: 스테이징 환경 (AWS Secrets Manager 사용)
- **Production**: 프로덕션 환경 (AWS Secrets Manager 사용)

## 📋 환경 설정

### 1. Development 환경

로컬 개발 시 `.env` 파일을 사용합니다.

```bash
# 환경 설정
ENVIRONMENT=development

# .env 파일 사용 (현재 설정된 값들)
# - 데이터베이스: 로컬 MySQL
# - 스토리지: MinIO (로컬 S3 호환)
# - OAuth: 로컬/개발 서버
```

**실행 방법:**
```bash
ENVIRONMENT=development cargo run
```

### 2. Staging 환경

AWS Secrets Manager를 사용하여 설정을 관리합니다.

**필요한 AWS Secret:**
- Secret Name: `staging/so-dod/cosmic-sync/config`
- Secret JSON: `aws-secret-staging.json` 참조

**설정 예시:**
```json
{
  "DB_HOST": "staging-db.example.com",
  "S3_BUCKET": "cosmic-sync-staging-files",
  "OAUTH_CLIENT_ID": "cosmic-sync-staging",
  "LOG_LEVEL": "info"
}
```

**실행 방법:**
```bash
ENVIRONMENT=staging cargo run
```

### 3. Production 환경

AWS Secrets Manager를 사용하여 설정을 관리합니다.

**필요한 AWS Secret:**
- Secret Name: `production/pop-os/cosmic-sync/config`
- Secret JSON: `aws-secret-production.json` 참조

**설정 예시:**
```json
{
  "DB_HOST": "prod-db.example.com",
  "S3_BUCKET": "cosmic-sync-production-files",
  "OAUTH_CLIENT_ID": "cosmic-sync-production",
  "LOG_LEVEL": "warn"
}
```

**실행 방법:**
```bash
ENVIRONMENT=production cargo run
```

## 🔑 AWS 설정

### IAM 권한

Staging/Production 환경에서는 다음 IAM 권한이 필요합니다:

```json
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Effect": "Allow",
            "Action": [
                "secretsmanager:GetSecretValue"
            ],
            "Resource": [
                "arn:aws:secretsmanager:us-east-2:*:secret:staging/so-dod/cosmic-sync/config-*",
                "arn:aws:secretsmanager:us-east-2:*:secret:production/pop-os/cosmic-sync/config-*"
            ]
        },
        {
            "Effect": "Allow",
            "Action": [
                "s3:GetObject",
                "s3:PutObject",
                "s3:DeleteObject",
                "s3:ListBucket"
            ],
            "Resource": [
                "arn:aws:s3:::cosmic-sync-staging-files",
                "arn:aws:s3:::cosmic-sync-staging-files/*",
                "arn:aws:s3:::cosmic-sync-production-files",
                "arn:aws:s3:::cosmic-sync-production-files/*"
            ]
        }
    ]
}
```

### AWS Secret 생성

**Staging 환경:**
```bash
aws secretsmanager create-secret \
    --name "staging/so-dod/cosmic-sync/config" \
    --description "Cosmic Sync Server staging configuration" \
    --secret-string file://aws-secret-staging.json \
    --region us-east-2
```

**Production 환경:**
```bash
aws secretsmanager create-secret \
    --name "production/pop-os/cosmic-sync/config" \
    --description "Cosmic Sync Server production configuration" \
    --secret-string file://aws-secret-production.json \
    --region us-east-2
```

## 📊 설정 우선순위

설정 로딩 우선순위는 다음과 같습니다:

1. **Development**: `.env` 파일 → 환경 변수 → 기본값
2. **Staging/Production**: AWS Secrets Manager → 환경 변수 → 기본값

## 🚀 Docker 배포

### Development
```dockerfile
ENV ENVIRONMENT=development
```

### Staging
```dockerfile
ENV ENVIRONMENT=staging
ENV AWS_REGION=us-east-2
```

### Production
```dockerfile
ENV ENVIRONMENT=production
ENV AWS_REGION=us-east-2
```

## 🔍 로그 확인

### Development
```bash
tail -f logs/cosmic-sync-server.log
```

### Staging/Production
```bash
# EC2/ECS 환경에서
tail -f /var/log/cosmic-sync-server.log

# CloudWatch Logs에서
aws logs tail /aws/ecs/cosmic-sync-server --follow
```

## 🛠️ 문제 해결

### 1. AWS Secrets Manager 접근 오류
```
❌ Failed to load configuration from AWS Secrets Manager
```

**해결 방법:**
- IAM 역할/사용자 권한 확인
- Secret 이름이 올바른지 확인
- AWS 리전 설정 확인

### 2. S3 접근 오류
```
❌ Failed to store file in S3
```

**해결 방법:**
- S3 버킷 존재 여부 확인
- IAM S3 권한 확인
- AWS 자격 증명 확인

### 3. 환경 변수 확인
```bash
# 현재 환경 확인
echo $ENVIRONMENT

# 로드된 설정 확인 (로그에서)
grep "Configuration loaded" logs/cosmic-sync-server.log
```

## 📝 환경별 차이점

| 항목 | Development | Staging | Production |
|------|-------------|---------|------------|
| 설정 소스 | `.env` 파일 | AWS Secrets | AWS Secrets |
| 데이터베이스 | 로컬 MySQL | RDS Staging | RDS Production |
| 파일 저장소 | 로컬 MinIO | S3 Staging | S3 Production |
| 로그 레벨 | `debug` | `info` | `warn` |
| 메트릭 | 비활성화 | 활성화 | 활성화 |
| 디버그 모드 | 활성화 | 비활성화 | 비활성화 | 