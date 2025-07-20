-- Migration: Add UNIQUE constraint on processes (environment_id, name)
-- This migration removes duplicate processes and adds a unique constraint

-- Step 1: Create a temporary table with the same structure as processes
CREATE TABLE processes_temp (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    command TEXT NOT NULL,
    working_dir TEXT NOT NULL,
    environment_id TEXT NOT NULL,
    pid INTEGER,
    status TEXT NOT NULL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    last_activity DATETIME,
    auto_restart BOOLEAN NOT NULL DEFAULT 0,
    health_check_url TEXT,
    health_check_interval INTEGER,
    depends_on TEXT, -- JSON array of dependency names
    env_vars TEXT, -- JSON object of environment variables
    FOREIGN KEY (environment_id) REFERENCES environments(id) ON DELETE CASCADE,
    UNIQUE(environment_id, name) -- Add the unique constraint
);

-- Step 2: Insert deduplicated data into the temporary table
-- Keep the most recently updated process for each (environment_id, name) combination
INSERT INTO processes_temp (
    id, name, command, working_dir, environment_id, pid, status,
    created_at, updated_at, last_activity, auto_restart,
    health_check_url, health_check_interval, depends_on, env_vars
)
SELECT 
    id, name, command, working_dir, environment_id, pid, status,
    created_at, updated_at, last_activity, auto_restart,
    health_check_url, health_check_interval, depends_on, env_vars
FROM processes p1
WHERE p1.updated_at = (
    SELECT MAX(p2.updated_at)
    FROM processes p2
    WHERE p2.environment_id = p1.environment_id 
    AND p2.name = p1.name
);

-- Step 3: Drop the original processes table
DROP TABLE processes;

-- Step 4: Rename the temporary table to processes
ALTER TABLE processes_temp RENAME TO processes;

-- Step 5: Recreate the indexes
CREATE INDEX idx_processes_environment ON processes(environment_id);
CREATE INDEX idx_processes_status ON processes(status);