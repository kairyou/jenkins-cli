use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;

use crate::config::DATA_DIR;
use crate::constants::{ParamType, MASKED_PASSWORD};
use crate::i18n::macros::t;
use crate::jenkins::{JenkinsJobParameter, ParamInfo};
use crate::migrations::{migrate_history, CURRENT_HISTORY_VERSION};
use crate::utils::{self, current_timestamp};

pub const HISTORY_FILE: &str = "history.toml";

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct History {
    pub entries: Vec<HistoryEntry>,
    #[serde(skip)]
    pub file_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct HistoryEntry {
    pub job_url: String,
    pub name: String,
    pub display_name: Option<String>,
    pub params: Option<HashMap<String, ParamInfo>>,
    pub created_at: Option<i64>,
    pub completed_at: Option<i64>,
}

impl History {
    /// Check if two history entries match
    #[doc(hidden)]
    fn matches_entry(entry: &HistoryEntry, info: &HistoryEntry) -> bool {
        // println!("matches_entry: {:?}, {:?}", entry, info);
        entry.job_url == info.job_url && entry.name == info.name
    }

    pub fn new() -> Result<Self> {
        let file_path = DATA_DIR.join(HISTORY_FILE);

        if let Err(_e) = migrate_history() {
            // eprintln!("Warning: Failed to migrate history: {}.", _e);
        }

        // auto create history file
        if !file_path.exists() {
            println!("Creating history file: {:?}", file_path);
            fs::File::create(&file_path).context("Failed to create history file")?;
        }
        // println!("history_file: {:?}", file_path);
        let mut history = Self {
            entries: vec![],
            version: Some(CURRENT_HISTORY_VERSION),
            file_path,
        };

        history.load_history()?;
        // println!("history: {:?}", history);

        Ok(history)
    }

    pub fn load_history(&mut self) -> Result<()> {
        let file = File::open(&self.file_path).context("Failed to open history file")?;
        let metadata = file.metadata().context("Failed to get file metadata")?;
        // println!("metadata: {:?}", metadata);

        // If the file is empty
        if metadata.len() == 0 {
            // println!("history file is empty");
            self.entries = vec![]; // *self = Self::default();
            return Ok(());
        }

        let mut content = String::new();
        let mut reader = BufReader::new(file);
        reader
            .read_to_string(&mut content)
            .context("Failed to read file content")?;
        // println!("load_history: {}", content);
        match toml::from_str::<History>(content.trim()) {
            Ok(file_history) => {
                self.entries = file_history.entries;
                self.version = file_history.version;
                Ok(())
            }
            Err(_e) => {
                // println!("Failed to parse history file: {}", _e);
                self.entries = vec![];
                Ok(())
            }
        }
    }

    pub fn save_history(&self) -> Result<()> {
        // println!("save_history: {:?}, {:?}", self.entries, self.file_path);
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.file_path)
            .context("Failed to open history file for writing")?;
        let mut writer = BufWriter::new(file);
        let content = toml::to_string(self).context("Failed to serialize history")?;
        writer
            .write_all(content.as_bytes())
            .context("Failed to write history to file")?;
        Ok(())
    }

    pub fn upsert_history(&mut self, entry: &mut HistoryEntry) -> Result<()> {
        // println!("upsert_history: {:?}", entry);
        entry.created_at = Some(current_timestamp());
        if let Some(existing_entry) = self.entries.iter_mut().find(|e| Self::matches_entry(e, entry)) {
            *existing_entry = entry.clone();
        } else {
            self.entries.push(entry.clone());
        }
        self.save_history()
    }

    /// get the history item by the job_url and name
    #[doc(hidden)]
    pub fn get_history(&self, info: &HistoryEntry, base_url: Option<&str>) -> Option<HistoryEntry> {
        // self.entries.iter().find(|e| Self::matches_entry(e, info)).cloned()
        let input_url = utils::simplify_url(base_url.unwrap_or(""));
        self.entries
            .iter()
            .filter(|e| input_url.is_empty() || e.job_url.contains(&input_url))
            .find(|e| Self::matches_entry(e, info))
            .cloned()
    }

    /// get the latest history item
    #[doc(hidden)]
    pub fn get_latest_history(&self, base_url: Option<&str>) -> Option<&HistoryEntry> {
        let items = self
            .entries
            .iter()
            .filter(|e| base_url.map_or(true, |url| e.job_url.contains(url)));
        // println!("entries: {:?}", self.entries);
        items.max_by_key(|entry| entry.created_at)
    }

    pub fn update_field<F>(&mut self, info: &HistoryEntry, update_fn: F) -> Result<()>
    where
        F: FnOnce(&mut HistoryEntry),
    {
        if let Some(entry) = self.entries.iter_mut().find(|e| Self::matches_entry(e, info)) {
            update_fn(entry);
            self.save_history()
        } else {
            anyhow::bail!("Entry not found");
        }
    }

    /// Display parameter differences and ask user to confirm usage of previous parameters
    pub async fn should_use_history_parameters(
        &self,
        history_item: &Option<HistoryEntry>,
        current_parameters: &[JenkinsJobParameter],
    ) -> bool {
        let current_param_names: HashSet<String> = current_parameters.iter().map(|param| param.name.clone()).collect();

        // create current parameter choices map, for checking if the choice value is still valid
        let current_param_choices: HashMap<String, Option<Vec<String>>> = current_parameters
            .iter()
            .map(|param| (param.name.clone(), param.choices.clone()))
            .collect();

        history_item.as_ref().is_some_and(|history| {
            let params = history.params.as_ref().unwrap();
            let datetime_str = history.created_at.map(|timestamp| {
                let utc_datetime = DateTime::from_timestamp(timestamp, 0).unwrap();
                // UTC => Local
                let local_datetime = utc_datetime.with_timezone(&Local);
                local_datetime.format("%Y-%m-%d %H:%M:%S").to_string()
            });

            println!(
                "{}{}",
                t!("last-build-params").bold(),
                datetime_str.map_or("".to_string(), |dt| format!(" ({})", dt))
            );

            // check if history parameters are consistent with current Jenkins config
            let history_param_names: HashSet<String> = params.keys().cloned().collect();

            // find new and missing parameters
            let new_params: Vec<String> = current_param_names.difference(&history_param_names).cloned().collect();
            let missing_params: Vec<String> = history_param_names.difference(&current_param_names).cloned().collect();

            // check invalid choice values
            let invalid_choices: Vec<String> = params
                .iter()
                .filter(|(_k, v)| v.r#type == ParamType::Choice)
                .filter(|(k, v)| {
                    if let Some(Some(choices)) = current_param_choices.get(k.as_str()) {
                        !choices.contains(&v.value)
                    } else {
                        false
                    }
                })
                .map(|(k, _)| k.clone())
                .collect();

            let has_param_changes = !new_params.is_empty() || !missing_params.is_empty() || !invalid_choices.is_empty();

            // display parameter changes info
            if has_param_changes {
                println!("{}", t!("params-changed-warning").yellow());
            }

            // display history parameter values
            for (key, param_info) in params.iter() {
                let display_value = if param_info.r#type == ParamType::Password {
                    MASKED_PASSWORD.to_string()
                } else {
                    param_info.value.clone()
                };

                if missing_params.contains(key) {
                    // deleted parameter - whole line red
                    println!("{} {}: {}", "-".red(), key.red().bold(), display_value.red());
                } else if invalid_choices.contains(key) {
                    // invalid choice value - whole line yellow + add mark
                    println!(
                        "{} {}: {} {}",
                        "!".yellow(),
                        key.yellow().bold(),
                        display_value.yellow(),
                        "<invalid>".yellow().italic()
                    );
                } else {
                    // unchanged parameter
                    println!("  {}: {}", key.bold(), display_value);
                }
            }

            // add "+" prefix to new parameters - whole line green
            for key in &new_params {
                println!("{} {}: {}", "+".green(), key.green().bold(), "<new>".green().italic());
            }

            // if there are parameter changes, clearly inform the user
            let prompt = if has_param_changes {
                t!("use-modified-last-build-params")
            } else {
                t!("use-last-build-params")
            };

            dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt(prompt)
                .default(!has_param_changes) // when there are changes, default to no, otherwise default to yes
                .interact()
                .unwrap_or_else(|_e| {
                    std::process::exit(0);
                })
        })
    }

    /// process history parameters and merge with latest config
    pub fn merge_parameters(
        history_item: &HistoryEntry,
        current_parameters: &[JenkinsJobParameter],
    ) -> HashMap<String, ParamInfo> {
        let current_param_names: HashSet<String> = current_parameters.iter().map(|param| param.name.clone()).collect();

        // merge history parameters with latest config
        let mut merged_params = history_item.params.clone().unwrap_or_default();

        // remove parameters that no longer exist
        merged_params.retain(|key, _| current_param_names.contains(key));

        // add new parameters (use default value)
        for param in current_parameters {
            if !merged_params.contains_key(&param.name) {
                if let Some(default_value) = &param.default_value {
                    merged_params.insert(
                        param.name.clone(),
                        ParamInfo {
                            value: default_value.clone(),
                            r#type: param.param_type.clone().unwrap_or(ParamType::String),
                        },
                    );
                }
            }
        }

        // fix invalid choice values
        for param in current_parameters {
            if let Some(choices) = &param.choices {
                if let Some(param_info) = merged_params.get_mut(&param.name) {
                    if param_info.r#type == ParamType::Choice && !choices.contains(&param_info.value) {
                        // if history value is no longer valid, use default value or first option
                        param_info.value = param
                            .default_value
                            .clone()
                            .unwrap_or_else(|| choices.first().map_or(String::new(), |c| c.clone()));
                    }
                }
            }
        }

        merged_params
    }
}
