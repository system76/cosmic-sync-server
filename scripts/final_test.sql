-- Final test for watcher_conditions
USE cosmic_sync;

-- Clean up any existing test data
DELETE FROM watcher_conditions WHERE watcher_id IN (SELECT id FROM watchers WHERE account_hash = 'test_account_hash');
DELETE FROM watchers WHERE account_hash = 'test_account_hash';
DELETE FROM watcher_groups WHERE account_hash = 'test_account_hash';
DELETE FROM accounts WHERE account_hash = 'test_account_hash';

-- Create test account first (required for foreign key)
INSERT INTO accounts (id, email, account_hash, name, created_at, updated_at, is_active) 
VALUES ('test-uuid-1234', 'test@example.com', 'test_account_hash', 'Test User', UNIX_TIMESTAMP(), UNIX_TIMESTAMP(), 1);

-- Find available IDs for manual assignment
SELECT COALESCE(MAX(id), 0) + 1 as next_group_id FROM watcher_groups;
SELECT COALESCE(MAX(id), 0) + 1 as next_watcher_id FROM watchers;

-- Insert test data with manual IDs
INSERT INTO watcher_groups (id, account_hash, device_hash, title, created_at, updated_at, is_active) 
VALUES (100, 'test_account_hash', 'test_device_hash', 'Test Group', NOW(), NOW(), 1);

INSERT INTO watchers (id, account_hash, group_id, folder, pattern, interval_seconds, is_recursive, created_at, updated_at) 
VALUES (100, 'test_account_hash', 100, '/test/folder', '*.json', 60, 1, UNIX_TIMESTAMP(), UNIX_TIMESTAMP());

-- Now insert watcher conditions (the main test)
INSERT INTO watcher_conditions (watcher_id, condition_type, `key`, `value`, operator, created_at, updated_at) VALUES
(100, 'union', 'extension', 'json', 'equals', UNIX_TIMESTAMP(), UNIX_TIMESTAMP()),
(100, 'union', 'extension', 'yaml', 'equals', UNIX_TIMESTAMP(), UNIX_TIMESTAMP()),
(100, 'union', 'extension', 'yml', 'equals', UNIX_TIMESTAMP(), UNIX_TIMESTAMP()),
(100, 'subtract', 'filename', 'cache.json', 'equals', UNIX_TIMESTAMP(), UNIX_TIMESTAMP()),
(100, 'subtract', 'filename', '*.bak', 'pattern', UNIX_TIMESTAMP(), UNIX_TIMESTAMP()),
(100, 'subtract', 'filename', '*.swo', 'pattern', UNIX_TIMESTAMP(), UNIX_TIMESTAMP()),
(100, 'subtract', 'filename', '*.swp', 'pattern', UNIX_TIMESTAMP(), UNIX_TIMESTAMP()),
(100, 'subtract', 'filename', '*.tmp', 'pattern', UNIX_TIMESTAMP(), UNIX_TIMESTAMP());

-- Show results
SELECT 'Test completed successfully!' as result;

SELECT 'Watcher info:' as section;
SELECT w.id, w.folder, w.pattern, COUNT(wc.id) as condition_count
FROM watchers w 
LEFT JOIN watcher_conditions wc ON w.id = wc.watcher_id 
WHERE w.account_hash = 'test_account_hash'
GROUP BY w.id, w.folder, w.pattern;

SELECT 'Watcher conditions:' as section;
SELECT wc.condition_type, wc.key, wc.value, wc.operator
FROM watcher_conditions wc
JOIN watchers w ON wc.watcher_id = w.id
WHERE w.account_hash = 'test_account_hash'
ORDER BY wc.condition_type, wc.key, wc.value;

SELECT 'Summary by condition type:' as section;
SELECT wc.condition_type, COUNT(*) as count
FROM watcher_conditions wc
JOIN watchers w ON wc.watcher_id = w.id
WHERE w.account_hash = 'test_account_hash'
GROUP BY wc.condition_type;

-- Test the complex conditions from user's example
SELECT 'Complex condition test:' as section;
SELECT 
    'Union conditions (should include):' as description,
    GROUP_CONCAT(CONCAT(wc.key, '=', wc.value) SEPARATOR ', ') as conditions
FROM watcher_conditions wc
JOIN watchers w ON wc.watcher_id = w.id
WHERE w.account_hash = 'test_account_hash' AND wc.condition_type = 'union';

SELECT 
    'Subtract conditions (should exclude):' as description,
    GROUP_CONCAT(CONCAT(wc.key, '=', wc.value) SEPARATOR ', ') as conditions
FROM watcher_conditions wc
JOIN watchers w ON wc.watcher_id = w.id
WHERE w.account_hash = 'test_account_hash' AND wc.condition_type = 'subtract';

COMMIT; 