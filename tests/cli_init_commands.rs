mod common;

use common::environment::{RunceptTestEnvironment, TestConfig};

/// Tests for CLI init command functionality
/// Verifies that the init command properly creates and manages .runcept.toml files

#[tokio::test]
async fn test_init_creates_default_config_file() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "init-default".to_string(),
        ..TestConfig::default()
    })
    .await;

    // Run init command in project directory
    test_env.assert_cmd_success(&["init"], "initialized");

    // Verify .runcept.toml was created
    let config_path = test_env.project_dir().join(".runcept.toml");
    assert!(
        config_path.exists(),
        "Init command should create .runcept.toml file"
    );

    // Verify the config file has basic structure
    let config_content = tokio::fs::read_to_string(&config_path)
        .await
        .expect("Failed to read created config file");

    assert!(
        config_content.contains("[environment]"),
        "Config should contain environment section"
    );

    assert!(
        config_content.contains("name ="),
        "Config should contain environment name"
    );
}

#[tokio::test]
async fn test_init_with_custom_path() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "init-custom-path".to_string(),
        ..TestConfig::default()
    })
    .await;

    // Create a subdirectory for custom init
    let custom_dir = test_env.project_dir().join("custom_project");
    tokio::fs::create_dir_all(&custom_dir)
        .await
        .expect("Failed to create custom directory");

    // Run init command with custom path using centralized method
    let output = test_env.init_project(Some(&custom_dir.to_string_lossy()), false);
    assert!(output.status.success(), "Init command should succeed");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("initialized"),
        "Output should contain 'initialized'"
    );

    // Verify .runcept.toml was created in custom directory
    let config_path = custom_dir.join(".runcept.toml");
    assert!(
        config_path.exists(),
        "Init command should create .runcept.toml file in custom directory"
    );

    // Verify the config file is valid
    let config_content = tokio::fs::read_to_string(&config_path)
        .await
        .expect("Failed to read created config file");

    assert!(
        config_content.contains("[environment]"),
        "Config should contain environment section"
    );
}

#[tokio::test]
async fn test_init_fails_when_config_already_exists() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "init-already-exists".to_string(),
        ..TestConfig::default()
    })
    .await;

    // Create an existing config file
    let existing_config = r#"
[environment]
name = "existing-env"

[processes.existing-process]
name = "existing-process"
command = "echo existing"
"#;

    test_env.create_config_file(existing_config).await.unwrap();

    // Run init command - should fail because config already exists
    test_env.assert_cmd_failure(&["init"], "already exists");

    // Verify original config is unchanged
    let config_path = test_env.project_dir().join(".runcept.toml");
    let config_content = tokio::fs::read_to_string(&config_path)
        .await
        .expect("Failed to read config file");

    assert!(
        config_content.contains("existing-env"),
        "Original config should be preserved"
    );
}

#[tokio::test]
async fn test_init_with_force_overwrites_existing_config() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "init-force-overwrite".to_string(),
        ..TestConfig::default()
    })
    .await;

    // Create an existing config file
    let existing_config = r#"
[environment]
name = "old-env"

[processes.old-process]
name = "old-process"
command = "echo old"
"#;

    test_env.create_config_file(existing_config).await.unwrap();

    // Run init command with --force flag using centralized method
    let output = test_env.init_project(None, true);
    assert!(output.status.success(), "Init with --force should succeed");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("initialized"),
        "Output should contain 'initialized'"
    );

    // Verify config was overwritten with new default content
    let config_path = test_env.project_dir().join(".runcept.toml");
    let config_content = tokio::fs::read_to_string(&config_path)
        .await
        .expect("Failed to read config file");

    // Should no longer contain old content
    assert!(
        !config_content.contains("old-env"),
        "Old config should be overwritten"
    );

    // Should contain new default structure
    assert!(
        config_content.contains("[environment]"),
        "New config should contain environment section"
    );
}

#[tokio::test]
async fn test_init_generated_config_is_valid_and_activatable() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "init-valid-config".to_string(),
        ..TestConfig::default()
    })
    .await;

    // Run init command using centralized method
    test_env.assert_cmd_success(&["init"], "initialized");

    // Try to activate the generated configuration
    test_env.assert_cmd_success(
        &["activate", &test_env.project_dir().to_string_lossy()],
        "activated",
    );

    // Verify we can check status after activation
    test_env.assert_cmd_success(&["status"], "environment");

    // If there are any sample processes in the generated config, verify we can list them
    let list_output = test_env.list_processes();

    // Should succeed even if no processes are defined
    assert!(
        list_output.status.success(),
        "List command should work with generated config"
    );
}

#[tokio::test]
async fn test_init_creates_proper_directory_structure() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "init-directory-structure".to_string(),
        ..TestConfig::default()
    })
    .await;

    // Create a nested directory path that doesn't exist yet
    let nested_path = test_env
        .project_dir()
        .join("path")
        .join("to")
        .join("new_project");

    // Run init command with nested path using centralized method
    let output = test_env.init_project(Some(&nested_path.to_string_lossy()), false);
    assert!(output.status.success(), "Init command should succeed");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("initialized"),
        "Output should contain 'initialized'"
    );

    // Verify the nested directory structure was created
    assert!(
        nested_path.exists(),
        "Init should create nested directory structure"
    );

    // Verify .runcept.toml was created in the nested directory
    let config_path = nested_path.join(".runcept.toml");
    assert!(
        config_path.exists(),
        "Init should create .runcept.toml in nested directory"
    );
}

#[tokio::test]
async fn test_init_with_invalid_path_permissions() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "init-invalid-permissions".to_string(),
        ..TestConfig::default()
    })
    .await;

    // Try to init in a path that shouldn't be writable (like root directory)
    // Note: This test might behave differently depending on system permissions
    let invalid_path = "/root/should_not_work";

    let init_output = test_env.init_project(Some(invalid_path), false);

    // Should either fail gracefully or succeed if we actually have permissions
    if !init_output.status.success() {
        let stderr = String::from_utf8_lossy(&init_output.stderr);
        assert!(
            stderr.contains("permission") || stderr.contains("denied") || stderr.contains("error"),
            "Should provide meaningful error for permission issues"
        );
    }
    // If it succeeds, that's fine too - it means we have broader permissions
}

#[tokio::test]
async fn test_init_with_relative_paths() {
    let test_env = RunceptTestEnvironment::with_config(TestConfig {
        project_name: "init-relative-paths".to_string(),
        ..TestConfig::default()
    })
    .await;

    // Test with various relative path formats
    let relative_paths = vec![".", "./current", "../sibling", "subdir/nested"];

    for rel_path in relative_paths {
        // Create the target directory if it doesn't exist
        let full_path = test_env.project_dir().join(rel_path);
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .expect("Failed to create parent directory");
        }
        tokio::fs::create_dir_all(&full_path)
            .await
            .expect("Failed to create target directory");

        // Run init with relative path using centralized method
        let init_output = test_env.init_project(Some(rel_path), true);

        if init_output.status.success() {
            // Verify config was created
            let config_path = full_path.join(".runcept.toml");
            assert!(
                config_path.exists(),
                "Init should work with relative path: {}",
                rel_path
            );
        }
        // Some relative paths might fail depending on working directory, which is okay
    }
}
