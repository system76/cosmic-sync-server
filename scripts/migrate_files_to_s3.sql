-- 데이터베이스에서 S3로 파일 마이그레이션을 위한 현황 파악 SQL

USE cosmic_sync;

-- 1. 현재 저장된 파일 정보 확인
SELECT 
    '=== 파일 저장 현황 ===' as info;

SELECT 
    COUNT(*) as total_files,
    SUM(CASE WHEN is_deleted = 0 THEN 1 ELSE 0 END) as active_files,
    SUM(CASE WHEN is_deleted = 1 THEN 1 ELSE 0 END) as deleted_files,
    MIN(FROM_UNIXTIME(created_time)) as earliest_file,
    MAX(FROM_UNIXTIME(created_time)) as latest_file
FROM files;

-- 2. 8월 1일 이후 파일들 확인
SELECT 
    '=== 8월 1일 이후 파일들 ===' as info;

SELECT 
    file_id,
    filename,
    file_path,
    ROUND(size / 1024, 2) as size_kb,
    FROM_UNIXTIME(created_time) as created_date,
    is_deleted,
    account_hash,
    device_hash
FROM files 
WHERE FROM_UNIXTIME(created_time) >= '2024-08-01'
ORDER BY created_time DESC;

-- 3. 파일 데이터 테이블의 실제 데이터 확인
SELECT 
    '=== 파일 데이터 저장 현황 ===' as info;

SELECT 
    COUNT(*) as files_with_data,
    ROUND(SUM(LENGTH(data)) / 1024 / 1024, 2) as total_size_mb,
    ROUND(AVG(LENGTH(data)) / 1024, 2) as avg_size_kb
FROM file_data;

-- 4. 파일 정보와 데이터가 연결된 현황
SELECT 
    '=== 파일 정보와 데이터 연결 현황 ===' as info;

SELECT 
    f.file_id,
    f.filename,
    f.size as metadata_size,
    LENGTH(fd.data) as actual_data_size,
    CASE 
        WHEN LENGTH(fd.data) = f.size THEN 'OK'
        WHEN fd.data IS NULL THEN 'NO_DATA'
        ELSE 'SIZE_MISMATCH'
    END as status,
    FROM_UNIXTIME(f.created_time) as created_date
FROM files f 
LEFT JOIN file_data fd ON f.file_id = fd.file_id
WHERE FROM_UNIXTIME(f.created_time) >= '2024-08-01' 
    AND f.is_deleted = 0
ORDER BY f.created_time DESC
LIMIT 20;

-- 5. 마이그레이션이 필요한 파일들
SELECT 
    '=== 마이그레이션 대상 파일들 ===' as info;

SELECT 
    COUNT(*) as migration_target_count,
    ROUND(SUM(LENGTH(fd.data)) / 1024 / 1024, 2) as total_data_size_mb
FROM files f 
JOIN file_data fd ON f.file_id = fd.file_id
WHERE f.is_deleted = 0; 