# S3 파일 저장 설정 가이드

이 서버는 현재 S3를 사용하여 파일을 저장하도록 설정되어 있습니다.

## 현재 설정 상태

`config/config.json`에서 `storage_type`이 `"s3"`로 설정되어 있어서 파일들이 S3에 저장됩니다.

## AWS 자격 증명 설정

S3를 사용하기 위해서는 AWS 자격 증명을 설정해야 합니다. 다음 중 하나의 방법을 선택하세요:

### 방법 1: 환경 변수 설정 (권장)

```bash
export AWS_REGION=us-east-2
export AWS_ACCESS_KEY_ID=your_access_key_id
export AWS_SECRET_ACCESS_KEY=your_secret_access_key
export S3_BUCKET=cosmic-sync-files
```

### 방법 2: AWS Credentials 파일 사용

`~/.aws/credentials` 파일을 생성하고 다음 내용을 추가:

```ini
[default]
aws_access_key_id = your_access_key_id
aws_secret_access_key = your_secret_access_key
```

`~/.aws/config` 파일도 생성:

```ini
[default]
region = us-east-2
```

### 방법 3: AWS IAM Role 사용 (EC2에서 실행할 때)

EC2 인스턴스에 S3 접근 권한이 있는 IAM Role을 연결하면 자동으로 자격 증명이 설정됩니다.

## S3 버킷 설정

1. AWS 콘솔에서 S3 버킷 `cosmic-sync-files`를 생성합니다.
2. 버킷의 권한 설정에서 적절한 접근 권한을 설정합니다.

## 필요한 IAM 권한

S3 버킷에 대해 다음 권한이 필요합니다:

```json
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Effect": "Allow",
            "Action": [
                "s3:GetObject",
                "s3:PutObject",
                "s3:DeleteObject",
                "s3:ListBucket",
                "s3:HeadBucket",
                "s3:CreateBucket"
            ],
            "Resource": [
                "arn:aws:s3:::cosmic-sync-files",
                "arn:aws:s3:::cosmic-sync-files/*"
            ]
        }
    ]
}
```

## 서버 실행

환경 변수를 설정한 후 서버를 실행:

```bash
sudo -E /home/yongjinchong/.cargo/bin/cargo run --bin cosmic-sync-server
```

## 문제 해결

### S3 연결 오류
- AWS 자격 증명이 올바른지 확인
- 버킷이 존재하는지 확인
- 네트워크 연결 상태 확인

### 권한 오류
- IAM 사용자/역할에 필요한 S3 권한이 있는지 확인
- 버킷 정책이 올바른지 확인

## 기존 데이터베이스 파일 마이그레이션

기존에 데이터베이스에 저장된 파일들을 S3로 마이그레이션하려면 별도의 스크립트가 필요합니다.

## 로그 확인

서버 로그에서 다음과 같은 메시지를 확인할 수 있습니다:

- S3 초기화 성공: "File storage initialized successfully: S3FileStorage"
- 파일 업로드: "Storing file data in S3: bucket=cosmic-sync-files, key=files/{file_id}, size={bytes}"
- S3 업로드 성공: "Successfully stored file in S3: files/{file_id}" 