use jenkins::config::CONFIG_FILE;
use jenkins::migrations::migrate_config_yaml_to_toml;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_migrate_yaml_config_to_toml() {
    let yaml_content = r#"
- name: "SIT"
  url: "http://jenkins1.com"
  user: ""
  token: ""
  includes: [ "frontend|(?i)\\bweb\\b", "test|job"]
  excludes: ["delete|demo"]
- name: "UAT"
  url: "http://jenkins2.com"
  user: ""
  token: ""
  includes:
    - "后端"
    - "backend"
"#;
    println!("yaml_content:\n{}", yaml_content);

    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join(CONFIG_FILE);
    let yaml_path = config_path.with_extension("yaml");
    fs::write(&yaml_path, yaml_content).unwrap();
    // println!("yaml_path:\n{}", yaml_path.display());

    migrate_config_yaml_to_toml(&config_path).unwrap();
    // println!("config_path:\n{}", config_path.display());

    let toml_content = fs::read_to_string(&config_path).unwrap();
    println!("toml_content:\n{}", toml_content);

    let expected_toml = r#"
[config]
# locale = "en-US"

[[jenkins]]
name = "SIT"
url = "http://jenkins1.com"
user = ""
token = ""
includes = ["frontend|(?i)\\bweb\\b", "test|job"]
excludes = ["delete|demo"]

[[jenkins]]
name = "UAT"
url = "http://jenkins2.com"
user = ""
token = ""
includes = ["后端", "backend"]
"#;

    assert_eq!(toml_content.trim(), expected_toml.trim());
    assert!(!yaml_path.exists());
    assert_eq!(
        fs::read_to_string(yaml_path.with_extension("yaml.bak")).unwrap(),
        yaml_content
    );
}

#[test]
fn test_migrate_yaml_config_preserves_strings_and_aliases() {
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join(CONFIG_FILE);
    let yaml_path = config_path.with_extension("yaml");
    fs::write(
        &yaml_path,
        r#"
- name: &name on
  url: 'https://jenkins.example.com'
  user: no
  token: '00123'
  includes: &filters [off, yes, 'true', 'frontend|\bweb\b']
- name: *name
  includes: *filters
"#,
    )
    .unwrap();

    migrate_config_yaml_to_toml(&config_path).unwrap();

    let content = fs::read_to_string(&config_path).unwrap();
    let config: toml::Value = toml::from_str(&content).unwrap();
    let entries = config["jenkins"].as_array().unwrap();
    assert_eq!(entries[0]["name"].as_str(), Some("on"));
    assert_eq!(entries[0]["user"].as_str(), Some("no"));
    assert_eq!(entries[0]["token"].as_str(), Some("00123"));
    assert_eq!(entries[1]["name"].as_str(), Some("on"));
    let filters: Vec<_> = entries[0]["includes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(filters, ["off", "yes", "true", "frontend|\\bweb\\b"]);
    assert_eq!(entries[0]["includes"], entries[1]["includes"]);
}

#[test]
fn test_invalid_yaml_config_keeps_original_file() {
    for content in ["- name: [unfinished", "- name: first\n  name: duplicate\n"] {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);
        let yaml_path = config_path.with_extension("yaml");
        fs::write(&yaml_path, content).unwrap();

        assert!(migrate_config_yaml_to_toml(&config_path).is_err());
        assert_eq!(fs::read_to_string(&yaml_path).unwrap(), content);
        assert!(!config_path.exists());
        assert!(!yaml_path.with_extension("yaml.bak").exists());
    }
}
