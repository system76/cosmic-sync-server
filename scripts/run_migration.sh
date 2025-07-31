#!/bin/bash

# MySQL 연결 정보 (config/config.json에서 가져옴)
DB_HOST="127.0.0.1"
DB_PORT="3306"
DB_USER="root"
DB_PASSWORD="recognizer"
DB_NAME="cosmic_sync"

# 마이그레이션 파일 경로
MIGRATION_FILE="$1"

if [ -z "$MIGRATION_FILE" ]; then
    echo "사용법: $0 <migration_file.sql>"
    echo "예시: $0 migrations/20250527174943_remove_ctime_mtime_from_files.sql"
    exit 1
fi

if [ ! -f "$MIGRATION_FILE" ]; then
    echo "오류: 마이그레이션 파일을 찾을 수 없습니다: $MIGRATION_FILE"
    exit 1
fi

echo "마이그레이션 실행 중: $MIGRATION_FILE"
echo "데이터베이스: $DB_NAME @ $DB_HOST:$DB_PORT"

# MySQL에서 마이그레이션 실행
mysql -h "$DB_HOST" -P "$DB_PORT" -u "$DB_USER" -p"$DB_PASSWORD" "$DB_NAME" < "$MIGRATION_FILE"

if [ $? -eq 0 ]; then
    echo "마이그레이션이 성공적으로 완료되었습니다."
else
    echo "마이그레이션 실행 중 오류가 발생했습니다."
    exit 1
fi