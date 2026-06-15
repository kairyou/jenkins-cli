use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;

use crate::config::DATA_DIR;
use crate::constants::{ParamType, MASKED_PASSWORD};
use crate::i18n::macros::t;
use crate::jenkins::{JenkinsJobParameter, ParamInfo};
use crate::prompt;
use crate::utils::{self, current_timestamp};

pub const PRESETS_FILE: &str = "presets.toml";
const CURRENT_PRESETS_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PresetStore {
    pub jobs: Vec<JobPresets>,
    #[serde(skip)]
    pub file_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JobPresets {
    pub service_url: String,
    pub job_url: String,
    pub job_name: String,
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_preset: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<ParameterPreset>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ParameterPreset {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub params: HashMap<String, ParamInfo>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct JobPresetIdentity {
    pub service_url: String,
    pub job_url: String,
    pub job_name: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ParameterSource {
    Preset(ParameterPreset),
    LastBuild,
    JenkinsDefault,
    ManagePresets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetBuildAction {
    Build,
    Edit,
    EditAndUpdate,
    EditAndSaveAs,
    Refill,
    Update,
    SaveAs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresetManageAction {
    Delete,
    Rename,
    Back,
}

impl PresetStore {
    pub fn new() -> Result<Self> {
        let file_path = DATA_DIR.join(PRESETS_FILE);
        if !file_path.exists() {
            fs::File::create(&file_path).context("Failed to create presets file")?;
        }

        let mut store = Self {
            jobs: vec![],
            version: Some(CURRENT_PRESETS_VERSION),
            file_path,
        };
        store.load_presets()?;
        Ok(store)
    }

    pub fn load_presets(&mut self) -> Result<()> {
        let file = File::open(&self.file_path).context("Failed to open presets file")?;
        let metadata = file.metadata().context("Failed to get presets file metadata")?;
        if metadata.len() == 0 {
            self.jobs = vec![];
            self.version = Some(CURRENT_PRESETS_VERSION);
            return Ok(());
        }

        let mut content = String::new();
        let mut reader = BufReader::new(file);
        reader
            .read_to_string(&mut content)
            .context("Failed to read presets file content")?;

        match toml::from_str::<PresetStore>(content.trim()) {
            Ok(file_store) => {
                self.jobs = file_store.jobs;
                self.version = file_store.version.or(Some(CURRENT_PRESETS_VERSION));
                Ok(())
            }
            Err(_) => {
                self.jobs = vec![];
                self.version = Some(CURRENT_PRESETS_VERSION);
                Ok(())
            }
        }
    }

    pub fn save_presets(&self) -> Result<()> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.file_path)
            .context("Failed to open presets file for writing")?;
        let mut writer = BufWriter::new(file);
        let content = toml::to_string(self).context("Failed to serialize presets")?;
        writer
            .write_all(content.as_bytes())
            .context("Failed to write presets file")?;
        Ok(())
    }

    pub fn get_job_presets(&self, identity: &JobPresetIdentity) -> Option<JobPresets> {
        self.jobs.iter().find(|job| Self::matches_job(job, identity)).cloned()
    }

    pub fn sorted_presets(&self, identity: &JobPresetIdentity) -> Vec<ParameterPreset> {
        let mut presets = self
            .get_job_presets(identity)
            .map(|job| job.presets)
            .unwrap_or_default();
        presets.sort_by(|a, b| {
            let a_used = a.last_used_at.is_some();
            let b_used = b.last_used_at.is_some();
            b_used
                .cmp(&a_used)
                .then_with(|| {
                    let a_time = a.last_used_at.or(a.updated_at).or(a.created_at).unwrap_or(0);
                    let b_time = b.last_used_at.or(b.updated_at).or(b.created_at).unwrap_or(0);
                    b_time.cmp(&a_time)
                })
                .then_with(|| a.name.cmp(&b.name))
        });
        presets
    }

    pub fn find_preset(&self, identity: &JobPresetIdentity, preset_name: &str) -> Option<ParameterPreset> {
        let name = preset_name.trim();
        self.get_job_presets(identity)
            .and_then(|job| job.presets.into_iter().find(|preset| preset.name == name))
    }

    pub fn upsert_preset(
        &mut self,
        identity: &JobPresetIdentity,
        preset_name: &str,
        params: HashMap<String, ParamInfo>,
    ) -> Result<()> {
        let name = preset_name.trim();
        if name.is_empty() {
            anyhow::bail!("Preset name cannot be empty");
        }

        let now = current_timestamp();
        let job = self.ensure_job(identity);
        job.last_preset = Some(name.to_string());

        if let Some(existing) = job.presets.iter_mut().find(|preset| preset.name == name) {
            existing.params = params;
            existing.updated_at = Some(now);
        } else {
            job.presets.push(ParameterPreset {
                name: name.to_string(),
                description: None,
                params,
                created_at: Some(now),
                updated_at: Some(now),
                last_used_at: None,
            });
        }

        self.save_presets()
    }

    pub fn mark_preset_used(&mut self, identity: &JobPresetIdentity, preset_name: &str) -> Result<()> {
        let now = current_timestamp();
        if let Some(job) = self.jobs.iter_mut().find(|job| Self::matches_job(job, identity)) {
            job.last_preset = Some(preset_name.to_string());
            if let Some(preset) = job.presets.iter_mut().find(|preset| preset.name == preset_name) {
                preset.last_used_at = Some(now);
                return self.save_presets();
            }
        }
        Ok(())
    }

    pub fn preset_exists(&self, identity: &JobPresetIdentity, preset_name: &str) -> bool {
        self.get_job_presets(identity)
            .map(|job| job.presets.iter().any(|preset| preset.name == preset_name))
            .unwrap_or(false)
    }

    pub fn delete_preset(&mut self, identity: &JobPresetIdentity, preset_name: &str) -> Result<bool> {
        let name = preset_name.trim();
        if let Some(job) = self.jobs.iter_mut().find(|job| Self::matches_job(job, identity)) {
            let before_len = job.presets.len();
            job.presets.retain(|preset| preset.name != name);
            if job.presets.len() == before_len {
                return Ok(false);
            }
            if job.last_preset.as_deref() == Some(name) {
                job.last_preset = job
                    .presets
                    .iter()
                    .max_by_key(|preset| {
                        preset
                            .last_used_at
                            .or(preset.updated_at)
                            .or(preset.created_at)
                            .unwrap_or(0)
                    })
                    .map(|preset| preset.name.clone());
            }
            self.save_presets()?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn rename_preset(&mut self, identity: &JobPresetIdentity, old_name: &str, new_name: &str) -> Result<bool> {
        let old_name = old_name.trim();
        let new_name = new_name.trim();
        if new_name.is_empty() {
            anyhow::bail!("Preset name cannot be empty");
        }

        if let Some(job) = self.jobs.iter_mut().find(|job| Self::matches_job(job, identity)) {
            if old_name != new_name && job.presets.iter().any(|preset| preset.name == new_name) {
                anyhow::bail!("Preset already exists");
            }

            if let Some(preset) = job.presets.iter_mut().find(|preset| preset.name == old_name) {
                preset.name = new_name.to_string();
                preset.updated_at = Some(current_timestamp());
                if job.last_preset.as_deref() == Some(old_name) {
                    job.last_preset = Some(new_name.to_string());
                }
                self.save_presets()?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn cleanup_obsolete_projects<T>(&mut self, existing_projects: &[T], service_url: &str) -> Result<Vec<String>>
    where
        T: AsRef<str>,
    {
        let set: HashSet<&str> = existing_projects.iter().map(|p| p.as_ref()).collect();
        let input_url = utils::simplify_url(service_url);
        let mut removed_names = Vec::new();

        self.jobs.retain(|job| {
            let url_matches = utils::simplify_url(&job.service_url) == input_url;
            let keep = !url_matches || set.contains(job.job_name.as_str());
            if !keep {
                removed_names.push(job.job_name.clone());
            }
            keep
        });

        if !removed_names.is_empty() {
            self.save_presets()?;
        }
        Ok(removed_names)
    }

    fn ensure_job(&mut self, identity: &JobPresetIdentity) -> &mut JobPresets {
        if let Some(idx) = self.jobs.iter().position(|job| Self::matches_job(job, identity)) {
            let job = &mut self.jobs[idx];
            job.job_name = identity.job_name.clone();
            job.display_name = identity.display_name.clone();
            return job;
        }

        self.jobs.push(JobPresets {
            service_url: utils::simplify_url(&identity.service_url),
            job_url: utils::simplify_url(&identity.job_url),
            job_name: identity.job_name.clone(),
            display_name: identity.display_name.clone(),
            last_preset: None,
            presets: vec![],
        });

        self.jobs.last_mut().expect("just pushed job presets")
    }

    fn matches_job(job: &JobPresets, identity: &JobPresetIdentity) -> bool {
        utils::simplify_url(&job.service_url) == utils::simplify_url(&identity.service_url)
            && utils::simplify_url(&job.job_url) == utils::simplify_url(&identity.job_url)
    }
}

pub fn print_params(params: &HashMap<String, ParamInfo>) {
    let mut items: Vec<_> = params.iter().collect();
    items.sort_by_key(|(key, _)| *key);

    for (key, param_info) in items {
        let display_value = if param_info.r#type == ParamType::Password {
            MASKED_PASSWORD.to_string()
        } else {
            param_info.value.clone()
        };
        print_param_line(key.bold().to_string(), display_value);
    }
}

fn print_param_line(key: String, value: String) {
    if value.contains('\n') {
        println!("{}: |", key);
        for line in value.lines() {
            println!("{}", line);
        }
        if value.ends_with('\n') {
            println!();
        }
    } else {
        println!("{}: {}", key, value);
    }
}

pub async fn select_parameter_source(
    store: &PresetStore,
    identity: &JobPresetIdentity,
    has_history: bool,
) -> Option<ParameterSource> {
    let presets = store.sorted_presets(identity);

    if presets.is_empty() {
        return if has_history {
            Some(ParameterSource::LastBuild)
        } else {
            Some(ParameterSource::JenkinsDefault)
        };
    }

    let mut items = Vec::new();
    let mut sources = Vec::new();

    for preset in presets {
        items.push(format!("{} ({})", preset.name, t!("parameter-source-preset")));
        sources.push(ParameterSource::Preset(preset));
    }

    if has_history {
        items.push(t!("parameter-source-last-build"));
        sources.push(ParameterSource::LastBuild);
    }

    items.push(t!("parameter-source-reenter"));
    sources.push(ParameterSource::JenkinsDefault);
    items.push(t!("parameter-source-manage-presets"));
    sources.push(ParameterSource::ManagePresets);

    let selection = prompt::handle_selection_opt(prompt::with_prompt_kind(prompt::PromptKind::FuzzySelectVim, || {
        FuzzySelect::with_theme(&ColorfulTheme::default())
            .with_prompt(t!("select-parameter-source"))
            .items(&items)
            .default(0)
            .vim_mode(true)
            .with_initial_text("")
            .interact_opt()
    }));

    selection.map(|idx| sources[idx].clone())
}

pub async fn select_preset_action(preset: &ParameterPreset) -> Option<PresetBuildAction> {
    println!("{}: {}", t!("parameter-preset").bold(), preset.name.bold().green());
    println!();
    print_params(&preset.params);
    prompt_preset_action(&[
        (t!("preset-action-build"), PresetBuildAction::Build),
        (
            t!("preset-action-edit-update-current"),
            PresetBuildAction::EditAndUpdate,
        ),
        (t!("preset-action-edit-save-as"), PresetBuildAction::EditAndSaveAs),
    ])
}

pub async fn select_after_edit_action(source_is_preset: bool) -> Option<PresetBuildAction> {
    let mut actions = vec![(t!("preset-action-build-once"), PresetBuildAction::Build)];
    if source_is_preset {
        actions.push((t!("preset-action-update-current"), PresetBuildAction::Update));
    }
    actions.push((t!("preset-action-save-as"), PresetBuildAction::SaveAs));
    prompt_preset_action(&actions)
}

pub async fn select_last_build_action() -> Option<PresetBuildAction> {
    prompt_preset_action(&[
        (t!("history-action-use-last"), PresetBuildAction::Build),
        (t!("history-action-edit-last"), PresetBuildAction::Edit),
        (t!("history-action-refill"), PresetBuildAction::Refill),
        (t!("preset-action-save-as"), PresetBuildAction::SaveAs),
    ])
}

pub async fn manage_presets(store: &mut PresetStore, identity: &JobPresetIdentity) -> Option<()> {
    loop {
        let action = select_manage_action()?;
        match action {
            PresetManageAction::Delete => {
                let preset = select_preset_for_management(store, identity, &t!("select-preset-to-delete"))?;
                let confirmed =
                    prompt::handle_confirm_opt(prompt::with_prompt_kind(prompt::PromptKind::Confirm, || {
                        dialoguer::Confirm::with_theme(&ColorfulTheme::default())
                            .with_prompt(t!("delete-preset-confirm", "name" => preset.name.clone()))
                            .default(false)
                            .show_default(true)
                            .wait_for_newline(false)
                            .interact_opt()
                    }))?;
                if confirmed {
                    match store.delete_preset(identity, &preset.name) {
                        Ok(true) => println!("{}", t!("parameter-preset-deleted", "name" => preset.name)),
                        Ok(false) => println!("{}", t!("preset-not-found", "name" => preset.name).yellow()),
                        Err(e) => eprintln!("{}", t!("update-preset-failed", "error" => e.to_string())),
                    }
                }
            }
            PresetManageAction::Rename => {
                let preset = select_preset_for_management(store, identity, &t!("select-preset-to-rename"))?;
                let new_name = prompt_preset_name(Some(&preset.name))?;
                match store.rename_preset(identity, &preset.name, &new_name) {
                    Ok(true) => println!(
                        "{}",
                        t!("parameter-preset-renamed", "old" => preset.name, "new" => new_name)
                    ),
                    Ok(false) => println!("{}", t!("preset-not-found", "name" => preset.name).yellow()),
                    Err(e) => eprintln!("{}", t!("update-preset-failed", "error" => e.to_string())),
                }
            }
            PresetManageAction::Back => return Some(()),
        }

        if store.sorted_presets(identity).is_empty() {
            return Some(());
        }
    }
}

pub fn prompt_preset_name(existing_name: Option<&str>) -> Option<String> {
    loop {
        let default_name = existing_name.unwrap_or("");
        let input = prompt::handle_input(prompt::with_prompt_kind(prompt::PromptKind::Input, || {
            dialoguer::Input::with_theme(&ColorfulTheme::default())
                .with_prompt(t!("parameter-preset-name"))
                .with_initial_text(default_name.to_string())
                .allow_empty(false)
                .interact_text()
        }))?;

        let name = input.trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
        println!("{}", t!("parameter-preset-name-empty").yellow());
    }
}

fn select_manage_action() -> Option<PresetManageAction> {
    let items = vec![
        t!("manage-preset-delete"),
        t!("manage-preset-rename"),
        t!("manage-preset-back"),
    ];
    let actions = [
        PresetManageAction::Delete,
        PresetManageAction::Rename,
        PresetManageAction::Back,
    ];
    let selection = prompt::handle_selection_opt(prompt::with_prompt_kind(prompt::PromptKind::FuzzySelectVim, || {
        FuzzySelect::with_theme(&ColorfulTheme::default())
            .with_prompt(t!("manage-presets"))
            .items(&items)
            .default(0)
            .vim_mode(true)
            .with_initial_text("")
            .interact_opt()
    }));

    selection.map(|idx| actions[idx])
}

fn select_preset_for_management(
    store: &PresetStore,
    identity: &JobPresetIdentity,
    prompt_text: &str,
) -> Option<ParameterPreset> {
    let presets = store.sorted_presets(identity);
    if presets.is_empty() {
        println!("{}", t!("no-parameter-presets").yellow());
        return None;
    }

    let items: Vec<String> = presets.iter().map(|preset| preset.name.clone()).collect();
    let selection = prompt::handle_selection_opt(prompt::with_prompt_kind(prompt::PromptKind::FuzzySelectVim, || {
        FuzzySelect::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt_text)
            .items(&items)
            .default(0)
            .vim_mode(true)
            .with_initial_text("")
            .interact_opt()
    }));

    selection.map(|idx| presets[idx].clone())
}

pub fn apply_preset_defaults(
    preset: &ParameterPreset,
    current_parameters: Vec<JenkinsJobParameter>,
) -> Vec<JenkinsJobParameter> {
    let history_entry = crate::jenkins::history::HistoryEntry {
        params: Some(preset.params.clone()),
        ..Default::default()
    };
    crate::jenkins::history::History::apply_history_defaults(&history_entry, current_parameters)
}

pub fn merge_preset_parameters(
    preset: &ParameterPreset,
    current_parameters: &[JenkinsJobParameter],
) -> HashMap<String, ParamInfo> {
    let history_entry = crate::jenkins::history::HistoryEntry {
        params: Some(preset.params.clone()),
        ..Default::default()
    };
    crate::jenkins::history::History::merge_parameters(&history_entry, current_parameters)
}

fn prompt_preset_action(actions: &[(String, PresetBuildAction)]) -> Option<PresetBuildAction> {
    println!();
    println!("{}:", t!("history-action-prompt"));
    for (label, _) in actions {
        println!("  {}", label);
    }

    loop {
        let input_hint = preset_action_input_hint(actions);
        let input = prompt::handle_input(prompt::with_prompt_kind(prompt::PromptKind::Input, || {
            dialoguer::Input::with_theme(&ColorfulTheme::default())
                .with_prompt(t!("preset-action-input", "keys" => input_hint.clone()))
                .allow_empty(true)
                .interact_text()
        }))?;

        let value = input.trim().to_lowercase();
        let value = if value.is_empty() { "y" } else { value.as_str() };
        if let Some((_, action)) = actions
            .iter()
            .find(|(_, available)| preset_action_key(*available) == value)
        {
            return Some(*action);
        }
        println!("{}", t!("preset-action-invalid", "keys" => input_hint).yellow());
    }
}

fn preset_action_input_hint(actions: &[(String, PresetBuildAction)]) -> String {
    actions
        .iter()
        .map(|(_, action)| preset_action_key(*action))
        .collect::<Vec<_>>()
        .join("/")
}

fn preset_action_key(action: PresetBuildAction) -> &'static str {
    match action {
        PresetBuildAction::Build => "y",
        PresetBuildAction::Edit => "e",
        PresetBuildAction::EditAndUpdate | PresetBuildAction::Update => "u",
        PresetBuildAction::EditAndSaveAs | PresetBuildAction::SaveAs => "s",
        PresetBuildAction::Refill => "r",
    }
}
