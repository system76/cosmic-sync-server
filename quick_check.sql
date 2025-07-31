-- Quick database check
USE cosmic_sync;
SELECT 'watcher_groups' as table_name, COUNT(*) as count FROM watcher_groups
UNION ALL  
SELECT 'watchers' as table_name, COUNT(*) as count FROM watchers
UNION ALL
SELECT 'watcher_conditions' as table_name, COUNT(*) as count FROM watcher_conditions;

SELECT 'Latest watcher_groups:' as info;
SELECT id, group_id, account_hash, title, created_at FROM watcher_groups ORDER BY id DESC LIMIT 3; 