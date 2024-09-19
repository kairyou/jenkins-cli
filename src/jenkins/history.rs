use anyhow::{Context, Result};
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;

use crate::utils::current_timestamp;

const HISTORY_FILE: &str = ".jenkins_history.toml";

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct HistoryEntry {
    pub job_url: String,
    pub name: String,
    pub display_name: Option<String>,
    pub user_params: Option<HashMap<String, String>>,
    pub created_at: Option<i64>,
    pub completed_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct History {
    entries: Vec<HistoryEntry>,
    #[serde(skip)]
    file_path: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct FileHistory {
    entries: Vec<HistoryEntry>,
}

impl History {
    /// Check if two history entries match
    #[doc(hidden)]
    fn matches_entry(entry: &HistoryEntry, info: &HistoryEntry) -> bool {
        entry.job_url == info.job_url && entry.name == info.name
    }

    pub fn new() -> Result<Self> {
        let mut file_path = home_dir().ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
        file_path.push(HISTORY_FILE);

        let _ = migrate_yaml_to_toml(&file_path);

        // auto create history file
        if !file_path.exists() {
            println!("Creating history file: {:?}", file_path);
            fs::File::create(&file_path).context("Failed to create history file")?;
        }
        // println!("history_file: {:?}", file_path);
        let mut history = Self {
            entries: Vec::new(),
            file_path,
        };
        history.load_history()?;
        // println!("History::new {:?}", history);
        Ok(history)
    }

    fn load_history(&mut self) -> Result<()> {
        let file = File::open(&self.file_path).context("Failed to open history file")?;
        let metadata = file.metadata().context("Failed to get file metadata")?;
        // println!("metadata: {:?}", metadata);

        // If the file is empty
        if metadata.len() == 0 {
            self.entries = Vec::new();
            return Ok(());
        }
        let mut file_content = String::new();
        let mut reader = BufReader::new(file);
        reader
            .read_to_string(&mut file_content)
            .context("Failed to read file content")?;
        // println!("load_history: {}", file_content);

        let file_history: FileHistory = toml::from_str(&file_content).context("解析历史文件失败")?;
        self.entries = file_history.entries;
        Ok(())
    }

    fn save_history(&self) -> Result<()> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.file_path)
            .context("Failed to open history file for writing")?;
        let mut writer = BufWriter::new(file);
        let toml_string = toml::to_string(&self.entries).context("Failed to serialize history")?;
        writer
            .write_all(toml_string.as_bytes())
            .context("Failed to write history to file")?;
        Ok(())
    }

    pub fn upsert_history(&mut self, entry: &mut HistoryEntry) -> Result<()> {
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
        self.entries
            .iter()
            .filter(|e| base_url.map_or(true, |url| e.job_url.contains(url)))
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

fn migrate_yaml_to_toml(config_path: &PathBuf) -> Result<()> {
    let yaml_path = config_path.with_extension("yaml");
    // println!("{}", yaml_path.display());
    if yaml_path.exists() {
        let yaml_content = fs::read_to_string(&yaml_path)?;
        let yaml_entries: Vec<HistoryEntry> = serde_yaml::from_str(&yaml_content)?;
        // println!("yaml_entries: {:?}", yaml_entries);
        let history = FileHistory { entries: yaml_entries };
        let toml_string = toml::to_string(&history)?;
        // println!("{}", toml_string);
        fs::write(config_path, toml_string)?;
        fs::remove_file(yaml_path)?;
    }
    Ok(())
}
