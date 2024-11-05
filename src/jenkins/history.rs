use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;

use crate::config::DATA_DIR;
use crate::jenkins::ParamInfo;
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
}
