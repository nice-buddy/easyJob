use easyjob_logging::init_logging;
use std::fs;
use tempfile::tempdir;
use tracing::info;

#[test]
fn test_logging_initialization_and_file_creation() {
    let dir = tempdir().unwrap();
    let guard = init_logging(dir.path(), false, "info").expect("logging init success");

    info!("Test logging message for verification");

    // Flush guard happens on drop or sync
    drop(guard);

    let files: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert!(!files.is_empty(), "Log files should not be empty");

    let log_file = &files[0];
    let file_name = log_file.file_name().unwrap().to_str().unwrap();
    assert!(
        file_name.starts_with("agent.log"),
        "Expected log file starting with agent.log, got: {}",
        file_name
    );

    let content = fs::read_to_string(log_file).expect("read log file content");
    assert!(
        content.contains("Test logging message for verification"),
        "Log file content does not contain expected message: {}",
        content
    );
}
