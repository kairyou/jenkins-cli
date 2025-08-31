use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::BufReader;

use crate::constants::{ParamType, DEFAULT_PARAM_VALUE};
pub mod client;
pub use client::ClientConfig;
#[doc(hidden)]
pub mod history;

#[derive(Debug, Clone)]
#[doc(hidden)]
pub enum Event {
    StopSpinner,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ParamInfo {
    pub value: String,
    #[serde(default = "default_param_type")]
    pub r#type: ParamType, // param_type
}

fn default_param_type() -> ParamType {
    ParamType::String
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct JenkinsJob {
    pub name: String,
    // #[serde(rename = "fullName")]
    // pub full_name: String, // folder/folder/test
    #[serde(rename = "displayName")]
    pub display_name: String,
    // #[serde(rename = "fullDisplayName")]
    // pub full_display_name: String, // folder » folder » test
    pub url: String,
    pub _class: String,
    pub jobs: Option<Vec<JenkinsJob>>,
}

#[derive(Deserialize, Serialize, Debug)]
struct JenkinsResponse {
    jobs: Vec<JenkinsJob>,
}

// job config
#[derive(Debug)]
pub struct JenkinsJobParameter {
    pub param_type: Option<ParamType>, // ParamType string, text, choice, boolean, password
    pub name: String,                  // parameter name
    pub description: Option<String>,   // parameter description
    pub default_value: Option<String>, // default value
    pub choices: Option<Vec<String>>,  // choices for select type
    pub trim: Option<bool>,            // trim string
    pub required: Option<bool>,        // CredentialsParameterDefinition
    pub credential_type: Option<String>, // CredentialsParameterDefinition
    pub project_name: Option<String>,  // RunParameterDefinition
    pub filter: Option<String>,        // RunParameterDefinition
}

// impl JenkinsJobParameter {
//     /// add is_trim method
//     pub fn is_trim(&self) -> bool {
//         self.trim.unwrap_or(false)
//     }
// }

static PARAMETER_DEFINITIONS: Lazy<HashMap<&'static [u8], ParamType>> = Lazy::new(|| {
    HashMap::from([
        (b"hudson.model.StringParameterDefinition" as &[u8], ParamType::String),
        (b"hudson.model.TextParameterDefinition", ParamType::Text),
        (b"hudson.model.ChoiceParameterDefinition", ParamType::Choice),
        (b"hudson.model.BooleanParameterDefinition", ParamType::Boolean),
        (b"hudson.model.PasswordParameterDefinition", ParamType::Password),
        // not supported
        // b"hudson.model.FileParameterDefinition"
        // b"com.cloudbees.plugins.credentials.CredentialsParameterDefinition"
        // b"hudson.model.RunParameterDefinition"
    ])
});

/// extract text from xml
fn extract_text(e: quick_xml::events::BytesText) -> String {
    e.unescape().unwrap_or_else(|_| Cow::from("")).trim().to_string()
}

/// Parse Jenkins job parameters from XML data
pub fn parse_jenkins_job_parameter(xml_data: &str) -> Vec<JenkinsJobParameter> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(BufReader::new(xml_data.as_bytes()));
    let mut buf = vec![];

    let mut parameters = vec![];
    let mut current_param = JenkinsJobParameter {
        param_type: None,
        name: String::new(),
        description: None,
        default_value: None,
        choices: None,
        trim: None,
        required: None,
        credential_type: None,
        filter: None,
        project_name: None,
    };

    let mut inside_choices = false;
    let mut choices = vec![];

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                val if PARAMETER_DEFINITIONS.contains_key(val) => {
                    current_param.param_type = Some(PARAMETER_DEFINITIONS[val].clone());
                }
                b"name" => {
                    if let Ok(Event::Text(e)) = reader.read_event_into(&mut buf) {
                        current_param.name = extract_text(e);
                    }
                }
                b"description" => {
                    if let Ok(Event::Text(e)) = reader.read_event_into(&mut buf) {
                        current_param.description = Some(extract_text(e));
                    }
                }
                b"defaultValue" => {
                    if let Ok(Event::Text(e)) = reader.read_event_into(&mut buf) {
                        let value = extract_text(e);
                        // println!("type: {:?}, name: {:?}", current_param.param_type, current_param.name);
                        if current_param.param_type == Some(ParamType::Password) {
                            current_param.default_value = Some(DEFAULT_PARAM_VALUE.to_string());
                        } else {
                            current_param.default_value = Some(value);
                        }
                    }
                }
                b"trim" => {
                    if let Ok(Event::Text(e)) = reader.read_event_into(&mut buf) {
                        current_param.trim = Some(extract_text(e) == "true");
                    }
                }
                b"credentialType" => {
                    if let Ok(Event::Text(e)) = reader.read_event_into(&mut buf) {
                        current_param.credential_type = Some(extract_text(e));
                    }
                }
                b"required" => {
                    if let Ok(Event::Text(e)) = reader.read_event_into(&mut buf) {
                        current_param.required = Some(extract_text(e) == "true");
                    }
                }
                b"filter" => {
                    if let Ok(Event::Text(e)) = reader.read_event_into(&mut buf) {
                        current_param.filter = Some(extract_text(e));
                    }
                }
                b"projectName" => {
                    if let Ok(Event::Text(e)) = reader.read_event_into(&mut buf) {
                        current_param.project_name = Some(extract_text(e));
                    }
                }
                b"choices" => {
                    inside_choices = true;
                }
                b"string" => {
                    if inside_choices {
                        if let Ok(Event::Text(e)) = reader.read_event_into(&mut buf) {
                            choices.push(extract_text(e));
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"choices" => {
                    inside_choices = false;
                    current_param.choices = Some(choices.clone());
                    choices.clear();
                }
                val if PARAMETER_DEFINITIONS.contains_key(val) => {
                    // println!("type: {:?}, name: {:?}", current_param.param_type, current_param.name);
                    parameters.push(current_param);
                    current_param = JenkinsJobParameter {
                        param_type: None,
                        name: String::new(),
                        description: None,
                        default_value: None,
                        choices: None,
                        trim: None,
                        credential_type: None,
                        required: None,
                        filter: None,
                        project_name: None,
                    };
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => panic!("Error: {:?}", e),
            _ => {}
        }
        buf.clear();
    }

    parameters
}
