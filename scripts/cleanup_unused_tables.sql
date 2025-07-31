-- Cleanup Unused Tables Script
-- This script removes deprecated tables that are no longer used in the current schema
-- NOTE: watcher_conditions and sync_logs tables are preserved as requested

-- Check if we're connected to the right database
SELECT DATABASE() as current_database;

-- Display tables that will be checked for removal
SHOW TABLES;

-- Drop unused tables if they exist (excluding watcher_conditions and sync_logs)

-- 1. Drop old users table (replaced by accounts table)
DROP TABLE IF EXISTS users;

-- 2. Drop old user_encryption_keys table (replaced by encryption_keys table)
DROP TABLE IF EXISTS user_encryption_keys;

-- 3. Drop old watcher_patterns table (functionality moved to watchers.pattern column)
DROP TABLE IF EXISTS watcher_patterns;

-- 4. Drop old watcher_devices table (functionality handled differently now)
DROP TABLE IF EXISTS watcher_devices;

-- Display remaining tables after cleanup
SELECT 'Tables after cleanup:' as status;
SHOW TABLES;

-- Show any remaining foreign key constraints that might need attention
SELECT 
    TABLE_NAME,
    COLUMN_NAME,
    CONSTRAINT_NAME,
    REFERENCED_TABLE_NAME,
    REFERENCED_COLUMN_NAME
FROM 
    INFORMATION_SCHEMA.KEY_COLUMN_USAGE 
WHERE 
    REFERENCED_TABLE_SCHEMA = DATABASE()
    AND REFERENCED_TABLE_NAME IN ('users', 'user_encryption_keys', 'watcher_patterns', 'watcher_devices')
ORDER BY 
    TABLE_NAME, COLUMN_NAME; 