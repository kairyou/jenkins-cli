use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value as JsonValue};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
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
#[derive(Debug, Default)]
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

/// Parse Jenkins job parameters from XML data.
pub fn parse_job_parameters_from_xml(xml_data: &str) -> Vec<JenkinsJobParameter> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(BufReader::new(xml_data.as_bytes()));
    let mut buf = vec![];

    let mut parameters = vec![];
    let mut current_param = JenkinsJobParameter::default();

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
                        current_param.default_value =
                            normalize_default_value(current_param.param_type.as_ref(), Some(value));
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
                        let choice = match reader.read_event_into(&mut buf) {
                            Ok(Event::Text(e)) => extract_text(e), // regular <string>value</string>
                            Ok(Event::End(ref end)) if end.name().as_ref() == b"string" => String::new(), // handles empty <string></string>
                            Ok(Event::Eof) => break, // stop on unexpected EOF
                            Ok(_) => String::new(),
                            Err(e) => panic!("Error: {:?}", e),
                        };
                        choices.push(choice);
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(ref e)) => match e.name().as_ref() {
                b"string" if inside_choices => {
                    choices.push(String::new()); // handles self-closing <string/>
                }
                _ => {}
            },
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"choices" => {
                    inside_choices = false;
                    current_param.choices = Some(std::mem::take(&mut choices));
                }
                val if PARAMETER_DEFINITIONS.contains_key(val) => {
                    // println!("type: {:?}, name: {:?}", current_param.param_type, current_param.name);
                    parameters.push(std::mem::take(&mut current_param));
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

#[derive(Debug, Default, Deserialize)]
struct JenkinsParametersApiResponse {
    #[serde(default)]
    actions: Vec<Option<JenkinsParametersAction>>,
    #[serde(default)]
    property: Vec<Option<JenkinsParametersProperty>>,
}

#[derive(Debug, Default, Deserialize)]
struct JenkinsParametersAction {
    #[serde(rename = "parameterDefinitions", default)]
    parameter_definitions: Vec<JenkinsApiParameterDefinition>,
}

#[derive(Debug, Default, Deserialize)]
struct JenkinsParametersProperty {
    #[serde(rename = "_class")]
    class: Option<String>,
    #[serde(rename = "parameterDefinitions", default)]
    parameter_definitions: Vec<JenkinsApiParameterDefinition>,
}

#[derive(Debug, Default, Deserialize)]
struct JenkinsApiParameterDefinition {
    #[serde(rename = "_class")]
    class: Option<String>,
    #[serde(rename = "type")]
    type_field: Option<String>,
    name: Option<String>,
    description: Option<String>,
    #[serde(rename = "defaultParameterValue")]
    default_parameter_value: Option<JenkinsApiDefaultParameterValue>,
    #[serde(default)]
    choices: Vec<JsonValue>,
    trim: Option<bool>,
    #[serde(rename = "credentialType")]
    credential_type: Option<String>,
    required: Option<bool>,
    filter: Option<String>,
    #[serde(rename = "projectName")]
    project_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct JenkinsApiDefaultParameterValue {
    value: Option<JsonValue>,
}

fn json_value_to_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::Null => None,
        JsonValue::String(s) => Some(s.clone()),
        JsonValue::Bool(b) => Some(b.to_string()),
        JsonValue::Number(n) => Some(n.to_string()),
        JsonValue::Array(_) | JsonValue::Object(_) => Some(value.to_string()),
    }
}

fn resolve_param_type(class_name: Option<&str>) -> Option<ParamType> {
    let name = class_name?;
    if let Some(param_type) = PARAMETER_DEFINITIONS.get(name.as_bytes()) {
        return Some(param_type.clone());
    }
    match name {
        "StringParameterDefinition" => Some(ParamType::String),
        "TextParameterDefinition" => Some(ParamType::Text),
        "ChoiceParameterDefinition" => Some(ParamType::Choice),
        "BooleanParameterDefinition" => Some(ParamType::Boolean),
        "PasswordParameterDefinition" => Some(ParamType::Password),
        _ => None,
    }
}

fn normalize_default_value(param_type: Option<&ParamType>, value: Option<String>) -> Option<String> {
    if matches!(param_type, Some(ParamType::Password)) {
        Some(DEFAULT_PARAM_VALUE.to_string())
    } else {
        value
    }
}

/// Parse Jenkins job parameters from the remote API JSON response.
pub fn parse_job_parameters_from_json(json_data: &JsonValue) -> Vec<JenkinsJobParameter> {
    let response: JenkinsParametersApiResponse = serde_json::from_value(json_data.clone()).unwrap_or_default();
    let mut parameters = Vec::new();
    let mut seen_names = HashSet::new();

    let mut push_definitions = |definitions: Vec<JenkinsApiParameterDefinition>| {
        for definition in definitions {
            let JenkinsApiParameterDefinition {
                class,
                type_field,
                name,
                description,
                default_parameter_value,
                choices,
                trim,
                credential_type,
                required,
                filter,
                project_name,
            } = definition;

            let Some(name) = name else { continue };
            if !seen_names.insert(name.clone()) {
                continue;
            }

            let param_type = resolve_param_type(class.as_deref()).or_else(|| resolve_param_type(type_field.as_deref()));
            if param_type.is_none() {
                continue;
            }

            let default_value = default_parameter_value
                .as_ref()
                .and_then(|value| value.value.as_ref())
                .and_then(json_value_to_string);

            let default_value = normalize_default_value(param_type.as_ref(), default_value);

            let parsed_choices: Vec<String> = choices.iter().filter_map(json_value_to_string).collect();
            let choices = if parsed_choices.is_empty() {
                None
            } else {
                Some(parsed_choices)
            };

            parameters.push(JenkinsJobParameter {
                param_type,
                name,
                description,
                default_value,
                choices,
                trim,
                required,
                credential_type,
                project_name,
                filter,
            });
        }
    };

    for action in response.actions.into_iter().flatten() {
        push_definitions(action.parameter_definitions);
    }

    for property in response.property.into_iter().flatten() {
        if property
            .class
            .as_ref()
            .map(|class_name| class_name == "hudson.model.ParametersDefinitionProperty")
            .unwrap_or(true)
        {
            push_definitions(property.parameter_definitions);
        }
    }

    parameters
}
