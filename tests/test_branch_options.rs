use jenkins::jenkins::client::{BranchOptionsInput, JenkinsClient};

#[test]
fn build_branch_options_keeps_manual_input_unique() {
    let manual_input = "[*] Manual input";
    let branches = vec![
        "main".to_string(),
        manual_input.to_string(),
        "release".to_string(),
        "main".to_string(),
    ];

    let options = JenkinsClient::build_branch_options(BranchOptionsInput {
        branches: &branches,
        default_branch: Some("main"),
        current_branch: Some("main"),
        manual_input,
    });

    assert_eq!(
        options,
        vec![manual_input.to_string(), "main".to_string(), "release".to_string(),]
    );
}

#[test]
fn build_branch_options_prioritizes_default_then_current() {
    let manual_input = "[*] Manual input";
    let branches = vec!["develop".to_string(), "main".to_string(), "release".to_string()];

    let options = JenkinsClient::build_branch_options(BranchOptionsInput {
        branches: &branches,
        default_branch: Some("release"),
        current_branch: Some("develop"),
        manual_input,
    });

    assert_eq!(
        options,
        vec![
            manual_input.to_string(),
            "release".to_string(),
            "develop".to_string(),
            "main".to_string(),
        ]
    );
}

#[test]
fn default_choice_selection_uses_matching_default_value() {
    let choices = vec!["sit".to_string(), "uat".to_string(), "prod".to_string()];

    assert_eq!(JenkinsClient::default_choice_selection(&choices, "uat"), 1);
}

#[test]
fn default_choice_selection_falls_back_to_first_choice() {
    let choices = vec!["sit".to_string(), "uat".to_string(), "prod".to_string()];

    assert_eq!(JenkinsClient::default_choice_selection(&choices, "dev"), 0);
}
