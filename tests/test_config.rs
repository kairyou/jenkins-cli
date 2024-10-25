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
}
