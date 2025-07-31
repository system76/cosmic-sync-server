-- Test script for watcher_conditions table
USE cosmic_sync;

-- Show current database
SELECT DATABASE() as current_database;

-- Check watcher_conditions table structure
DESCRIBE watcher_conditions;

-- First, let's check if we have any watchers
SELECT COUNT(*) as watcher_count FROM watchers;

-- Create a sample watcher for testing if none exists
INSERT IGNORE INTO watcher_groups (id, account_hash, device_hash, title, created_at, updated_at, is_active) 
VALUES (1, 'test_account_hash', 'test_device_hash', 'Test Group', NOW(), NOW(), 1);

INSERT IGNORE INTO watchers (id, account_hash, group_id, folder, pattern, interval_seconds, is_recursive, created_at, updated_at) 
VALUES (1, 'test_account_hash', 1, '/test/folder', '*.json', 60, 1, UNIX_TIMESTAMP(), UNIX_TIMESTAMP());

-- Show current watchers for reference
SELECT * FROM watchers LIMIT 5;

-- Insert sample watcher conditions (assuming watcher_id = 1)
INSERT INTO watcher_conditions (watcher_id, condition_type, `key`, `value`, operator, created_at, updated_at) VALUES
(1, 'union', 'extension', 'json', 'equals', UNIX_TIMESTAMP(), UNIX_TIMESTAMP()),
(1, 'union', 'extension', 'yaml', 'equals', UNIX_TIMESTAMP(), UNIX_TIMESTAMP()),
(1, 'union', 'extension', 'yml', 'equals', UNIX_TIMESTAMP(), UNIX_TIMESTAMP()),
(1, 'subtract', 'filename', 'cache.json', 'equals', UNIX_TIMESTAMP(), UNIX_TIMESTAMP()),
(1, 'subtract', 'filename', '*.bak', 'pattern', UNIX_TIMESTAMP(), UNIX_TIMESTAMP()),
(1, 'subtract', 'filename', '*.swo', 'pattern', UNIX_TIMESTAMP(), UNIX_TIMESTAMP()),
(1, 'subtract', 'filename', '*.swp', 'pattern', UNIX_TIMESTAMP(), UNIX_TIMESTAMP()),
(1, 'subtract', 'filename', '*.tmp', 'pattern', UNIX_TIMESTAMP(), UNIX_TIMESTAMP());

-- Show inserted conditions with readable timestamps
SELECT 
    id,
    watcher_id,
    condition_type,
    `key`,
    `value`,
    operator,
    FROM_UNIXTIME(created_at) as created_at,
    FROM_UNIXTIME(updated_at) as updated_at
FROM watcher_conditions 
ORDER BY watcher_id, condition_type, `key`, `value`;

-- Test query: Count conditions by watcher_id
SELECT 
    watcher_id,
    condition_type,
    `key`,
    `value`,
    COUNT(*) as count
FROM watcher_conditions 
GROUP BY watcher_id, condition_type, `key`, `value`
ORDER BY watcher_id, condition_type, `key`, `value`;

-- Summary: Total conditions by type
SELECT 
    condition_type,
    COUNT(*) as total_conditions
FROM watcher_conditions 
GROUP BY condition_type;

-- Test foreign key constraint: Join with watchers table
SELECT 
    w.id as watcher_id,
    w.folder,
    w.pattern,
    wc.condition_type,
    wc.key,
    wc.value,
    wc.operator
FROM watchers w
LEFT JOIN watcher_conditions wc ON w.id = wc.watcher_id
ORDER BY w.id, wc.condition_type, wc.key;

COMMIT; 