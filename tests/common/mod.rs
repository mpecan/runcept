use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, Once};

static INIT: Once = Once::new();
static BINARY_PATHS: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

/// Build binaries once and cache their paths for all tests
pub fn ensure_binaries_built() {
    INIT.call_once(|| {
        println!("Building binaries once for all tests...");
        
        // Build the project
        let build_output = std::process::Command::new("cargo")
            .args(&["build", "--bins"])
            .output()
            .expect("Failed to build binaries");
        
        if !build_output.status.success() {
            panic!("Failed to build binaries: {}", String::from_utf8_lossy(&build_output.stderr));
        }
        
        // Get the target directory
        let target_dir = std::process::Command::new("cargo")
            .args(&["metadata", "--format-version=1", "--no-deps"])
            .output()
            .expect("Failed to get cargo metadata")
            .stdout;
        
        let metadata: serde_json::Value = serde_json::from_slice(&target_dir)
            .expect("Failed to parse cargo metadata");
        
        let target_directory = metadata["target_directory"].as_str()
            .expect("Failed to get target directory");
        
        let debug_dir = PathBuf::from(target_directory).join("debug");
        
        // Store binary paths
        let mut paths = HashMap::new();
        paths.insert("runcept".to_string(), debug_dir.join("runcept"));
        
        // Verify binaries exist
        for (name, path) in &paths {
            if !path.exists() {
                panic!("Binary {} not found at {}", name, path.display());
            }
        }
        
        *BINARY_PATHS.lock().unwrap() = Some(paths);
        println!("Binaries built and cached successfully");
    });
}

/// Get the path to a binary
pub fn get_binary_path(name: &str) -> PathBuf {
    ensure_binaries_built();
    let paths = BINARY_PATHS.lock().unwrap();
    paths.as_ref().unwrap().get(name).unwrap().clone()
}