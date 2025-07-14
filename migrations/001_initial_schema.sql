-- Create environments table
CREATE TABLE environments (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    project_path TEXT NOT NULL,
    config_path TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    last_activity DATETIME,
    inactivity_timeout INTEGER, -- timeout in seconds
    auto_shutdown BOOLEAN NOT NULL DEFAULT 0
);

-- Create processes table
CREATE TABLE processes (
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
    FOREIGN KEY (environment_id) REFERENCES environments(id) ON DELETE CASCADE
);

-- Create activity_logs table
CREATE TABLE activity_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    process_id TEXT,
    environment_id TEXT,
    activity_type TEXT NOT NULL,
    timestamp DATETIME NOT NULL,
    details TEXT, -- JSON object with additional details
    FOREIGN KEY (process_id) REFERENCES processes(id) ON DELETE CASCADE,
    FOREIGN KEY (environment_id) REFERENCES environments(id) ON DELETE CASCADE
);

-- Create indexes for better performance
CREATE INDEX idx_processes_environment ON processes(environment_id);
CREATE INDEX idx_processes_status ON processes(status);
CREATE INDEX idx_environments_status ON environments(status);
CREATE INDEX idx_activity_logs_timestamp ON activity_logs(timestamp);
CREATE INDEX idx_activity_logs_process ON activity_logs(process_id);
CREATE INDEX idx_activity_logs_environment ON activity_logs(environment_id);