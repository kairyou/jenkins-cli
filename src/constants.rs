use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamType {
    String,
    Text,
    Choice,
    Boolean,
    Password,
}

// impl ParamType {
//     pub fn as_str(&self) -> &'static str {
//         match self {
//             ParamType::String => "string",
//             ParamType::Text => "text",
//             ParamType::Choice => "choice",
//             ParamType::Boolean => "boolean",
//             ParamType::Password => "password",
//         }
//     }
// }

/// Default parameter value (password)
pub const DEFAULT_PARAM_VALUE: &str = "<DEFAULT>";

/// Masked value for password fields
pub const MASKED_PASSWORD: &str = "*******";

/// Jenkins job types that can be built manually
pub const JENKINS_BUILDABLE_TYPES: [&str; 2] = [
    "hudson.model.FreeStyleProject",                  // Freestyle project
    "org.jenkinsci.plugins.workflow.job.WorkflowJob", // Pipeline project
];

/// Jenkins folder type for organizing jobs
pub const JENKINS_FOLDER_TYPE: &str = "com.cloudbees.hudson.plugins.folder.Folder";

/// Jenkins job types that are auto-built (e.g. multibranch pipelines)
pub const JENKINS_AUTO_BUILD_TYPES: [&str; 2] = [
    "jenkins.branch.OrganizationFolder",
    "org.jenkinsci.plugins.workflow.multibranch.WorkflowMultiBranchProject",
];
