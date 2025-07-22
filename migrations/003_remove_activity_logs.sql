-- Remove activity logs table and related indexes
-- This migration removes the activity_logs table as we've moved to file-based logging only

-- Drop indexes first
DROP INDEX IF EXISTS idx_activity_logs_timestamp;
DROP INDEX IF EXISTS idx_activity_logs_process;
DROP INDEX IF EXISTS idx_activity_logs_environment;

-- Drop the activity_logs table
DROP TABLE IF EXISTS activity_logs;