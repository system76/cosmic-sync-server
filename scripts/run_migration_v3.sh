#!/bin/bash

# 방안 3: 테이블 스키마 정규화 마이그레이션 스크립트
# 이 스크립트는 files 테이블을 서버 ID 체계로 변환합니다.

echo "==============================================="
echo "방안 3: 테이블 스키마 정규화 마이그레이션"
echo "==============================================="
echo ""
echo "이 마이그레이션은 다음 작업을 수행합니다:"
echo "1. files 테이블 백업"
echo "2. 서버 ID 컬럼 추가"
echo "3. 클라이언트 ID를 서버 ID로 매핑"
echo "4. 외래키 제약조건 추가"
echo ""
echo "⚠️  경고: 이 작업은 대용량 데이터의 경우 시간이 걸릴 수 있습니다."
echo ""

read -p "계속 진행하시겠습니까? (y/N): " confirm
if [[ $confirm != "y" && $confirm != "Y" ]]; then
    echo "마이그레이션이 취소되었습니다."
    exit 0
fi

# MySQL 연결 정보
DB_USER="root"
DB_PASS="recognizer"
DB_HOST="127.0.0.1"
DB_PORT="3306"
DB_NAME="cosmic_sync"

# 마이그레이션 파일
MIGRATION_FILE="../migrations/20250612_normalize_files_table_server_ids.sql"

echo ""
echo "데이터베이스에 연결 중..."
mysql -u${DB_USER} -p${DB_PASS} -h${DB_HOST} -P${DB_PORT} --ssl-mode=DISABLED ${DB_NAME} < ${MIGRATION_FILE}

if [ $? -eq 0 ]; then
    echo ""
    echo "✅ 마이그레이션이 성공적으로 완료되었습니다!"
    echo ""
    echo "다음 단계:"
    echo "1. 서버를 재시작하세요"
    echo "2. 클라이언트가 정상적으로 파일을 업로드하는지 확인하세요"
    echo "3. 문제가 발생하면 files_backup_20250612 테이블에서 복구할 수 있습니다"
else
    echo ""
    echo "❌ 마이그레이션 중 오류가 발생했습니다!"
    echo "데이터베이스 로그를 확인하세요."
fi 