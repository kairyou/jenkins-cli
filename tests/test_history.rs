use jenkins::constants::ParamType;
use jenkins::jenkins::history::*;
use jenkins::migrations::{migrate_history_yaml_to_toml, migrate_to_v1};
use serde_json::Value as JsonValue;
use std::fs;
use tempfile::tempdir;

fn setup_test_history() -> (History, tempfile::TempDir) {
    let temp_dir = tempdir().unwrap();
    let file_path = temp_dir.path().join(HISTORY_FILE);
    let history = History {
        entries: vec![],
        file_path,
        version: None,
    };
    (history, temp_dir)
}

#[test]
fn test_new_history() {
    let (history, _temp_dir) = setup_test_history();
    assert!(history.entries.is_empty());
    // println!("test_new_history: {:?}", history);
}

#[test]
fn test_upsert_history() {
    let (mut history, _temp_dir) = setup_test_history();
    let mut entry = HistoryEntry {
        job_url: "http://example.com/job1".to_string(),
        name: "Job1".to_string(),
        display_name: Some("Test Job 1".to_string()),
        params: None,
        created_at: None,
        completed_at: None,
    };

    history.upsert_history(&mut entry).unwrap();
    assert_eq!(history.entries.len(), 1);

    // update existing entry
    entry.display_name = Some("Updated Job 1".to_string());
    history.upsert_history(&mut entry).unwrap();
    assert_eq!(history.entries.len(), 1);
    assert_eq!(history.entries[0].display_name, Some("Updated Job 1".to_string()));
    println!("test_upsert_history: {:?}", history);
}

#[test]
fn test_get_history() {
    let (mut history, _temp_dir) = setup_test_history();
    let entry = HistoryEntry {
        job_url: "http://example.com/job1".to_string(),
        name: "Job1".to_string(),
        display_name: Some("Test Job 1".to_string()),
        params: None,
        created_at: Some(1000),
        completed_at: None,
    };
    history.upsert_history(&mut entry.clone()).unwrap();

    let result = history.get_history(&entry, Some("http://example.com"));
    assert!(result.is_some());
    assert_eq!(result.unwrap().name, "Job1");
}

#[test]
fn test_get_latest_history() {
    let (mut history, _temp_dir) = setup_test_history();
    let entry1 = HistoryEntry {
        job_url: "http://example.com/job1".to_string(),
        name: "Job1".to_string(),
        created_at: Some(1000),
        ..Default::default()
    };
    let entry2 = HistoryEntry {
        job_url: "http://example.com/job2".to_string(),
        name: "Job2".to_string(),
        created_at: Some(2000),
        ..Default::default()
    };
    history.upsert_history(&mut entry1.clone()).unwrap();
    history.upsert_history(&mut entry2.clone()).unwrap();

    let latest = history.get_latest_history(Some("http://example.com"));
    assert!(latest.is_some());
    assert_eq!(latest.unwrap().name, "Job2");
}

#[test]
fn test_update_field() {
    let (mut history, _temp_dir) = setup_test_history();
    let mut entry = HistoryEntry {
        job_url: "http://example.com/job1".to_string(),
        name: "Job1".to_string(),
        ..Default::default()
    };
    history.upsert_history(&mut entry).unwrap();

    history.update_field(&entry, |e| e.completed_at = Some(3000)).unwrap();

    let updated = history.get_history(&entry, None).unwrap();
    assert_eq!(updated.completed_at, Some(3000));
}

#[test]
fn test_migrate_history_v0_yaml() {
    let temp_dir = tempdir().unwrap();
    let yaml_path = temp_dir.path().join("test_history.yaml");
    let toml_path = yaml_path.with_extension("toml");

    // Create test YAML file
    let yaml_content = r#"
- job_url: "http://example.com/job1"
  name: "Job1"
  display_name: "Test Job 1"
  created_at: 1000
  user_params:
    IS_DEBUG: "true"
    APP_ENV: sit
    GIT_BRANCH: main
"#;
    fs::write(&yaml_path, yaml_content).unwrap();

    migrate_history_yaml_to_toml(&yaml_path, &toml_path).unwrap();

    assert!(!yaml_path.exists());
    assert!(toml_path.exists());

    // Verify TOML content
    let toml_content = fs::read_to_string(&toml_path).unwrap();
    // println!("test_migrate toml_content: `{}`", toml_content);
    let mut json_value: JsonValue = toml::from_str(&toml_content).unwrap();

    migrate_to_v1(&mut json_value).unwrap();

    let toml_content = toml::to_string(&json_value).unwrap();
    println!("test_migrate toml_content: `{}`", toml_content);

    let history: History = toml::from_str(&toml_content).unwrap();

    let entry = &history.entries[0];
    println!("test_migrate params: {:?}", entry.params);
    assert_eq!(entry.name, "Job1");
    assert_eq!(entry.job_url, "http://example.com/job1");

    let params = entry.params.as_ref().unwrap();
    assert_eq!(params.len(), 3); // params length

    let git_branch = params.get("GIT_BRANCH").unwrap();
    assert_eq!(git_branch.value, "main");
    assert_eq!(git_branch.r#type, ParamType::String);
}
