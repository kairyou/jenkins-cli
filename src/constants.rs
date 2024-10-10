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
