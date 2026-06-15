use jenkins::constants::ParamType;
use jenkins::jenkins::presets::*;
use jenkins::jenkins::ParamInfo;
use std::collections::HashMap;
use tempfile::tempdir;

const SERVICE_URL: &str = "http://example.com";
const JOB_URL: &str = "http://example.com/job/frontend";

fn setup_store() -> (PresetStore, tempfile::TempDir) {
    let temp_dir = tempdir().unwrap();
    let file_path = temp_dir.path().join(PRESETS_FILE);
    let store = PresetStore {
        jobs: vec![],
        file_path,
        version: Some(1),
    };
    (store, temp_dir)
}

fn identity(job_url: &str, job_name: &str) -> JobPresetIdentity {
    JobPresetIdentity {
        service_url: SERVICE_URL.to_string(),
        job_url: job_url.to_string(),
        job_name: job_name.to_string(),
        display_name: Some(job_name.to_string()),
    }
}

fn params(branch: &str) -> HashMap<String, ParamInfo> {
    HashMap::from([
        (
            "BRANCH".to_string(),
            ParamInfo {
                value: branch.to_string(),
                r#type: ParamType::String,
            },
        ),
        (
            "ENV".to_string(),
            ParamInfo {
                value: "sit".to_string(),
                r#type: ParamType::Choice,
            },
        ),
    ])
}

fn special_params() -> HashMap<String, ParamInfo> {
    HashMap::from([
        (
            "branch name".to_string(),
            ParamInfo {
                value: "feature/space name".to_string(),
                r#type: ParamType::String,
            },
        ),
        (
            "deploy.env".to_string(),
            ParamInfo {
                value: "uat".to_string(),
                r#type: ParamType::Choice,
            },
        ),
    ])
}

#[test]
fn upsert_preset_creates_and_updates_within_job_scope() {
    let (mut store, _temp_dir) = setup_store();
    let id = identity(JOB_URL, "frontend");

    store.upsert_preset(&id, "release-main", params("main")).unwrap();
    assert_eq!(store.jobs.len(), 1);
    assert_eq!(store.jobs[0].presets.len(), 1);
    assert_eq!(store.jobs[0].last_preset.as_deref(), Some("release-main"));
    assert_eq!(store.jobs[0].presets[0].params["BRANCH"].value.as_str(), "main");

    store.upsert_preset(&id, "release-main", params("release/1.0")).unwrap();
    assert_eq!(store.jobs.len(), 1);
    assert_eq!(store.jobs[0].presets.len(), 1);
    assert_eq!(store.jobs[0].presets[0].params["BRANCH"].value.as_str(), "release/1.0");
}

#[test]
fn same_preset_name_is_allowed_for_different_jobs() {
    let (mut store, _temp_dir) = setup_store();
    let frontend = identity(JOB_URL, "frontend");
    let backend = identity("http://example.com/job/backend", "backend");

    store.upsert_preset(&frontend, "release-main", params("main")).unwrap();
    store
        .upsert_preset(&backend, "release-main", params("backend-main"))
        .unwrap();

    assert_eq!(store.jobs.len(), 2);
    assert!(store.preset_exists(&frontend, "release-main"));
    assert!(store.preset_exists(&backend, "release-main"));

    let frontend_presets = store.sorted_presets(&frontend);
    let backend_presets = store.sorted_presets(&backend);
    assert_eq!(frontend_presets[0].params["BRANCH"].value, "main");
    assert_eq!(backend_presets[0].params["BRANCH"].value, "backend-main");
}

#[test]
fn find_preset_matches_within_job_scope() {
    let (mut store, _temp_dir) = setup_store();
    let frontend = identity(JOB_URL, "frontend");
    let backend = identity("http://example.com/job/backend", "backend");

    store.upsert_preset(&frontend, "release-main", params("main")).unwrap();
    store
        .upsert_preset(&backend, "release-main", params("backend-main"))
        .unwrap();

    let frontend_preset = store.find_preset(&frontend, "release-main").unwrap();
    let backend_preset = store.find_preset(&backend, "release-main").unwrap();

    assert_eq!(frontend_preset.params["BRANCH"].value, "main");
    assert_eq!(backend_preset.params["BRANCH"].value, "backend-main");
    assert!(store.find_preset(&frontend, "missing").is_none());
}

#[test]
fn persists_preset_names_and_params_with_spaces_and_special_chars() {
    let (mut store, _temp_dir) = setup_store();
    let id = identity(JOB_URL, "frontend");
    let preset_name = "UAT 发布";

    store.upsert_preset(&id, preset_name, special_params()).unwrap();

    let mut loaded = PresetStore {
        jobs: vec![],
        file_path: store.file_path.clone(),
        version: None,
    };
    loaded.load_presets().unwrap();

    let preset = loaded.find_preset(&id, preset_name).unwrap();
    assert_eq!(preset.name, preset_name);
    assert_eq!(preset.params["branch name"].value, "feature/space name");
    assert_eq!(preset.params["deploy.env"].value, "uat");
}

#[test]
fn mark_preset_used_updates_last_used_and_sorts_first() {
    let (mut store, _temp_dir) = setup_store();
    let id = identity(JOB_URL, "frontend");

    store.upsert_preset(&id, "release-main", params("main")).unwrap();
    store.upsert_preset(&id, "hotfix", params("hotfix/1")).unwrap();
    store.mark_preset_used(&id, "release-main").unwrap();

    let presets = store.sorted_presets(&id);
    assert_eq!(presets[0].name, "release-main");
    assert!(presets[0].last_used_at.is_some());
}

#[test]
fn delete_preset_removes_it_and_updates_last_preset() {
    let (mut store, _temp_dir) = setup_store();
    let id = identity(JOB_URL, "frontend");

    store.upsert_preset(&id, "release-main", params("main")).unwrap();
    store.upsert_preset(&id, "hotfix", params("hotfix/1")).unwrap();

    assert!(store.delete_preset(&id, "hotfix").unwrap());
    assert!(store.find_preset(&id, "hotfix").is_none());

    let job = store.get_job_presets(&id).unwrap();
    assert_eq!(job.last_preset.as_deref(), Some("release-main"));
    assert!(!store.delete_preset(&id, "missing").unwrap());
}

#[test]
fn rename_preset_preserves_params_and_rejects_conflicts() {
    let (mut store, _temp_dir) = setup_store();
    let id = identity(JOB_URL, "frontend");

    store.upsert_preset(&id, "release-main", params("main")).unwrap();
    store.upsert_preset(&id, "hotfix", params("hotfix/1")).unwrap();

    assert!(store.rename_preset(&id, "hotfix", "hotfix-prod").unwrap());
    assert!(store.find_preset(&id, "hotfix").is_none());
    assert_eq!(
        store.find_preset(&id, "hotfix-prod").unwrap().params["BRANCH"].value,
        "hotfix/1"
    );
    assert!(!store.rename_preset(&id, "missing", "new-name").unwrap());
    assert!(store.rename_preset(&id, "hotfix-prod", "release-main").is_err());
}

#[test]
fn cleanup_obsolete_projects_removes_matching_service_only() {
    let (mut store, _temp_dir) = setup_store();
    let frontend = identity(JOB_URL, "frontend");
    let backend = identity("http://example.com/job/backend", "backend");
    let other_service = JobPresetIdentity {
        service_url: "http://other.example.com".to_string(),
        job_url: "http://other.example.com/job/frontend".to_string(),
        job_name: "frontend".to_string(),
        display_name: None,
    };

    store.upsert_preset(&frontend, "release-main", params("main")).unwrap();
    store.upsert_preset(&backend, "release-main", params("main")).unwrap();
    store
        .upsert_preset(&other_service, "release-main", params("main"))
        .unwrap();

    let removed = store.cleanup_obsolete_projects(&["frontend"], SERVICE_URL).unwrap();
    assert_eq!(removed, vec!["backend".to_string()]);
    assert_eq!(store.jobs.len(), 2);
    assert!(store.get_job_presets(&frontend).is_some());
    assert!(store.get_job_presets(&backend).is_none());
    assert!(store.get_job_presets(&other_service).is_some());
}
