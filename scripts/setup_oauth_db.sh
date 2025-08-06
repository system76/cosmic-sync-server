#!/bin/bash

# OAuth 테이블 설정 스크립트
# 사용법: ./scripts/setup_oauth_db.sh

set -e

# 색상 정의
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}Cosmic Sync OAuth Database Setup${NC}"
echo -e "${GREEN}========================================${NC}"

# 환경 변수에서 DB 정보 읽기 (기본값 제공)
DB_HOST="${DB_HOST:-localhost}"
DB_PORT="${DB_PORT:-3306}"
DB_NAME="${DB_NAME:-cosmic_sync}"
DB_USER="${DB_USER:-root}"

echo -e "\n${YELLOW}Database Configuration:${NC}"
echo "  Host: $DB_HOST"
echo "  Port: $DB_PORT"
echo "  Database: $DB_NAME"
echo "  User: $DB_USER"

# 비밀번호 입력 받기
echo -e "\n${YELLOW}Enter MySQL password for user '$DB_USER':${NC}"
read -s DB_PASSWORD

# MySQL 연결 테스트
echo -e "\n${YELLOW}Testing database connection...${NC}"
if mysql -h "$DB_HOST" -P "$DB_PORT" -u "$DB_USER" -p"$DB_PASSWORD" -e "SELECT 1" > /dev/null 2>&1; then
    echo -e "${GREEN}✓ Database connection successful${NC}"
else
    echo -e "${RED}✗ Failed to connect to database${NC}"
    exit 1
fi

# 데이터베이스 생성 (없는 경우)
echo -e "\n${YELLOW}Creating database if not exists...${NC}"
mysql -h "$DB_HOST" -P "$DB_PORT" -u "$DB_USER" -p"$DB_PASSWORD" -e "CREATE DATABASE IF NOT EXISTS $DB_NAME;"
echo -e "${GREEN}✓ Database '$DB_NAME' ready${NC}"

# 테이블 생성 스크립트 실행
echo -e "\n${YELLOW}Setting up OAuth tables...${NC}"
if mysql -h "$DB_HOST" -P "$DB_PORT" -u "$DB_USER" -p"$DB_PASSWORD" "$DB_NAME" < scripts/setup_oauth_tables.sql; then
    echo -e "${GREEN}✓ OAuth tables created/updated successfully${NC}"
else
    echo -e "${RED}✗ Failed to create tables${NC}"
    exit 1
fi

# 테이블 상태 확인
echo -e "\n${YELLOW}Checking table status...${NC}"
mysql -h "$DB_HOST" -P "$DB_PORT" -u "$DB_USER" -p"$DB_PASSWORD" "$DB_NAME" -e "
    SELECT TABLE_NAME, TABLE_ROWS, CREATE_TIME, UPDATE_TIME 
    FROM information_schema.tables 
    WHERE TABLE_SCHEMA = '$DB_NAME' 
    AND TABLE_NAME IN ('accounts', 'auth_tokens', 'encryption_keys')
    ORDER BY TABLE_NAME;
"

# 계정 수 확인
echo -e "\n${YELLOW}Current database statistics:${NC}"
mysql -h "$DB_HOST" -P "$DB_PORT" -u "$DB_USER" -p"$DB_PASSWORD" "$DB_NAME" -e "
    SELECT 
        (SELECT COUNT(*) FROM accounts) as 'Total Accounts',
        (SELECT COUNT(*) FROM accounts WHERE is_active = TRUE) as 'Active Accounts',
        (SELECT COUNT(*) FROM auth_tokens WHERE is_valid = TRUE AND expires_at > UNIX_TIMESTAMP()) as 'Valid Tokens';
"

# 최근 계정 확인
echo -e "\n${YELLOW}Recent accounts (last 5):${NC}"
mysql -h "$DB_HOST" -P "$DB_PORT" -u "$DB_USER" -p"$DB_PASSWORD" "$DB_NAME" -e "
    SELECT 
        SUBSTRING(account_hash, 1, 20) as 'Account Hash (partial)',
        email as 'Email',
        name as 'Name',
        user_type as 'Type',
        FROM_UNIXTIME(created_at) as 'Created',
        is_active as 'Active'
    FROM accounts 
    ORDER BY created_at DESC 
    LIMIT 5;
"

echo -e "\n${GREEN}========================================${NC}"
echo -e "${GREEN}✓ OAuth database setup completed!${NC}"
echo -e "${GREEN}========================================${NC}"

# 개발 환경 확인
if [ "$ENVIRONMENT" = "development" ] || [ "$NODE_ENV" = "development" ]; then
    echo -e "\n${YELLOW}Development environment detected.${NC}"
    echo -e "Would you like to insert test data? (y/n)"
    read -r response
    if [[ "$response" =~ ^[Yy]$ ]]; then
        echo -e "${YELLOW}Inserting test account...${NC}"
        mysql -h "$DB_HOST" -P "$DB_PORT" -u "$DB_USER" -p"$DB_PASSWORD" "$DB_NAME" -e "
            INSERT IGNORE INTO accounts (
                id, 
                account_hash, 
                email, 
                name, 
                user_id, 
                user_type,
                password_hash,
                salt,
                created_at, 
                updated_at, 
                last_login, 
                is_active
            ) VALUES (
                'test-account-id-1234',
                '209f313bf330cf40fe89fae938babbeba7ec95d31237f77cf19de418c0d50a0a',
                'test@example.com',
                'Test User',
                'test@example.com',
                'oauth',
                '',
                '',
                UNIX_TIMESTAMP(),
                UNIX_TIMESTAMP(),
                UNIX_TIMESTAMP(),
                TRUE
            );
        "
        echo -e "${GREEN}✓ Test data inserted${NC}"
    fi
fi

echo -e "\n${GREEN}Next steps:${NC}"
echo "1. Ensure your .env file has the correct OAuth settings"
echo "2. Restart the Cosmic Sync server: cargo run"
echo "3. Test OAuth login at: http://localhost:8080/oauth/login"
