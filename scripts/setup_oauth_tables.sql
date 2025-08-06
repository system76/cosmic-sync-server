-- OAuth 관련 테이블 설정 스크립트
-- 실행: mysql -u username -p cosmic_sync < scripts/setup_oauth_tables.sql

-- 1. accounts 테이블 생성 (이미 존재하는 경우 무시)
CREATE TABLE IF NOT EXISTS accounts (
    id VARCHAR(36) PRIMARY KEY,
    account_hash VARCHAR(64) UNIQUE NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    name VARCHAR(255),
    user_id VARCHAR(255),
    user_type VARCHAR(50) DEFAULT 'oauth',
    password_hash VARCHAR(255),
    salt VARCHAR(255),
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    last_login BIGINT,
    is_active BOOLEAN DEFAULT TRUE,
    INDEX idx_account_hash (account_hash),
    INDEX idx_email (email),
    INDEX idx_user_id (user_id),
    INDEX idx_created_at (created_at),
    INDEX idx_last_login (last_login)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- 2. auth_tokens 테이블 생성 (이미 존재하는 경우 무시)
CREATE TABLE IF NOT EXISTS auth_tokens (
    token_id VARCHAR(36) PRIMARY KEY,
    account_hash VARCHAR(64) NOT NULL,
    access_token VARCHAR(255) UNIQUE NOT NULL,
    token_type VARCHAR(50) DEFAULT 'Bearer',
    refresh_token VARCHAR(255),
    created_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    is_valid BOOLEAN DEFAULT TRUE,
    scope VARCHAR(255),
    INDEX idx_account_hash (account_hash),
    INDEX idx_access_token (access_token),
    INDEX idx_expires_at (expires_at),
    INDEX idx_is_valid (is_valid),
    FOREIGN KEY (account_hash) REFERENCES accounts(account_hash) ON DELETE CASCADE ON UPDATE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- 3. encryption_keys 테이블 생성 (이미 존재하는 경우 무시)
CREATE TABLE IF NOT EXISTS encryption_keys (
    account_hash VARCHAR(64) PRIMARY KEY,
    encryption_key VARCHAR(255) NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    FOREIGN KEY (account_hash) REFERENCES accounts(account_hash) ON DELETE CASCADE ON UPDATE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- 4. 기존 테이블 인덱스 최적화 (이미 존재하는 경우 무시)
-- accounts 테이블
ALTER TABLE accounts ADD INDEX IF NOT EXISTS idx_email_hash (email, account_hash);
ALTER TABLE accounts ADD INDEX IF NOT EXISTS idx_active_login (is_active, last_login);

-- auth_tokens 테이블
ALTER TABLE auth_tokens ADD INDEX IF NOT EXISTS idx_valid_expires (is_valid, expires_at);
ALTER TABLE auth_tokens ADD INDEX IF NOT EXISTS idx_hash_valid (account_hash, is_valid);

-- 5. 테스트 데이터 삽입 (개발 환경용)
-- 주의: 프로덕션에서는 이 부분을 제거하세요
/*
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
*/

-- 6. 데이터 확인 쿼리
SELECT 'Accounts table count:' as info, COUNT(*) as count FROM accounts
UNION ALL
SELECT 'Auth tokens count:', COUNT(*) FROM auth_tokens
UNION ALL
SELECT 'Active accounts:', COUNT(*) FROM accounts WHERE is_active = TRUE
UNION ALL
SELECT 'Valid tokens:', COUNT(*) FROM auth_tokens WHERE is_valid = TRUE AND expires_at > UNIX_TIMESTAMP();

-- 7. 테이블 구조 확인
DESCRIBE accounts;
DESCRIBE auth_tokens;
DESCRIBE encryption_keys;
