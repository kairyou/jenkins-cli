use clap::{Arg, Command};
use colored::*;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use regex::Regex;
use std::collections::HashMap;
use tokio::sync::mpsc;

mod config;
mod constants;
mod env_checks;
mod flow;
mod i18n;
mod interrupts;
mod jenkins;
mod migrations;
mod models;
mod prompt;
mod spinner;
mod terminal;
mod update;
mod utils;

// use crate::i18n::I18n;
use crate::i18n::macros::t;
use crate::{
    config::{initialize_config, CONFIG},
    env_checks::check_unsupported_terminal,
    flow::{handle_back_and_route, RouteAction, StepTracker},
    interrupts::{handle_ctrl_c, spawn_ctrl_c_key_listener, CtrlCPhase, CTRL_C},
    jenkins::{
        client::JenkinsClient,
        history::{History, HistoryEntry},
        presets::{self, JobPresetIdentity, ParameterSource, PresetBuildAction, PresetStore},
        ClientConfig, Event,
    },
    models::JenkinsConfig,
    update::{check_update, notify_if_update_available, precheck_update_status},
    utils::{clear_screen, current_timestamp, format_url, prepare_terminal_for_exit},
};

#[tokio::main]
async fn main() {
    let matches = Command::new("jenkins")
        .version(env!("CARGO_PKG_VERSION"))
        // .author("Your Name <your.email@example.com>")
        .about("A CLI tool for deploying projects using Jenkins")
        .arg(
            Arg::new("url")
                .short('U') // cargo run -- -u 123
                .long("url")
                .value_name("URL")
                .help("Sets the Jenkins URL")
                .required(false),
        )
        .arg(
            Arg::new("user")
                .short('u')
                .long("user")
                .value_name("USER")
                .help("Sets the Jenkins User ID")
                .required(false),
        )
        .arg(
            Arg::new("token")
                .short('t')
                .long("token")
                .value_name("TOKEN")
                .help("Sets the Jenkins API Token")
                .required(false),
        )
        .arg(
            Arg::new("cookie")
                .short('c')
                .long("cookie")
                .value_name("COOKIE")
                .help("Sets the Jenkins auth cookie (e.g. jwt_token=...)")
                .required(false),
        )
        .arg(
            Arg::new("preset")
                .long("preset")
                .value_name("PRESET")
                .help("Uses a saved parameter preset for the specified Jenkins job URL")
                .required(false),
        )
        .get_matches();
    check_unsupported_terminal();

    precheck_update_status();
    notify_if_update_available(); // before loading config

    let (global_config, service_step_enabled) = initialize_config(&matches).await.unwrap();
    let should_check_update = global_config.check_update.unwrap_or(true);

    clear_screen();

    if should_check_update {
        tokio::spawn(async {
            check_update().await;
        });
    }

    let (ctrlc_tx, ctrlc_rx) = mpsc::unbounded_channel();
    // Background key listener for queue/build phase
    tokio::spawn(async move {
        spawn_ctrl_c_key_listener(ctrlc_tx).await;
    });
    // Global Ctrl+C handler (selection uses dialoguer, build uses cancel flow)
    tokio::spawn(async move {
        handle_ctrl_c(ctrlc_rx).await;
    });

    // main logic - loop to allow returning to service selection
    let preset_arg = matches.get_one::<String>("preset").cloned();

    loop {
        if menu(service_step_enabled, preset_arg.as_deref()).await {
            clear_screen();
            if let Err(e) = config::select_jenkins_service().await {
                eprintln!("Failed to select service: {}", e);
                std::process::exit(1);
            }
            continue;
        }
        break;
    }

    if CTRL_C.phase() == CtrlCPhase::Cancelling {
        // Keep the process alive until the cancel flow completes.
        CTRL_C.wait_for_cancel().await;
        return;
    }
    CTRL_C.set_app_running(false);
    prepare_terminal_for_exit();
}

// actions

fn filter_projects(projects: Vec<jenkins::JenkinsJob>, config: &JenkinsConfig) -> Vec<jenkins::JenkinsJob> {
    fn compile_patterns(patterns: Option<&Vec<String>>) -> Vec<Regex> {
        patterns
            .map(|patterns| {
                patterns
                    .iter()
                    .map(|pattern| Regex::new(pattern).expect("Invalid regex"))
                    .collect()
            })
            .unwrap_or_default()
    }

    let includes = compile_patterns(Some(&config.includes));
    let excludes = compile_patterns(Some(&config.excludes));

    projects
        .into_iter()
        .filter(|project| {
            let display_name = &project.display_name;
            let name = &project.name;

            let matches_include =
                includes.is_empty() || includes.iter().any(|re| re.is_match(display_name) || re.is_match(name));
            let matches_exclude = excludes.iter().any(|re| re.is_match(display_name) || re.is_match(name));
            matches_include && !matches_exclude
        })
        .collect()
}

fn handle_menu_back(steps: &mut StepTracker) -> Option<bool> {
    match handle_back_and_route(steps, &t!("bye")) {
        RouteAction::ReturnService => Some(true),
        RouteAction::ContinueProject => {
            clear_screen();
            None
        }
    }
}

async fn resolve_user_parameters(
    presets: &mut PresetStore,
    identity: &JobPresetIdentity,
    source: ParameterSource,
    history_item: Option<&HistoryEntry>,
    current_parameters: Vec<jenkins::JenkinsJobParameter>,
) -> Option<(HashMap<String, jenkins::ParamInfo>, Option<String>)> {
    match source {
        ParameterSource::Preset(preset) => {
            let action = presets::select_preset_action(&preset).await?;
            match action {
                PresetBuildAction::Build => {
                    let params = presets::merge_preset_parameters(&preset, &current_parameters);
                    Some((params, Some(preset.name)))
                }
                PresetBuildAction::Edit => {
                    let parameter_definitions = presets::apply_preset_defaults(&preset, current_parameters);
                    let params = JenkinsClient::prompt_job_parameters(parameter_definitions).await?;
                    Some((params, None))
                }
                PresetBuildAction::EditAndUpdate => {
                    let parameter_definitions = presets::apply_preset_defaults(&preset, current_parameters);
                    let params = JenkinsClient::prompt_job_parameters(parameter_definitions).await?;
                    handle_preset_save_action(presets, identity, &preset.name, params, PresetBuildAction::Update)
                }
                PresetBuildAction::EditAndSaveAs => {
                    let parameter_definitions = presets::apply_preset_defaults(&preset, current_parameters);
                    let params = JenkinsClient::prompt_job_parameters(parameter_definitions).await?;
                    handle_preset_save_action(presets, identity, &preset.name, params, PresetBuildAction::SaveAs)
                }
                PresetBuildAction::Refill => {
                    let params = JenkinsClient::prompt_job_parameters(current_parameters).await?;
                    let post_action = presets::select_after_edit_action(false).await?;
                    handle_preset_save_action(presets, identity, "", params, post_action)
                }
                PresetBuildAction::Update => {
                    let params = presets::merge_preset_parameters(&preset, &current_parameters);
                    if let Err(e) = presets.upsert_preset(identity, &preset.name, params.clone()) {
                        eprintln!("{}", t!("update-preset-failed", "error" => e.to_string()));
                    } else {
                        println!("{}", t!("parameter-preset-updated", "name" => preset.name.clone()));
                    }
                    Some((params, Some(preset.name)))
                }
                PresetBuildAction::SaveAs => {
                    let params = presets::merge_preset_parameters(&preset, &current_parameters);
                    handle_preset_save_action(presets, identity, &preset.name, params, PresetBuildAction::SaveAs)
                }
            }
        }
        ParameterSource::LastBuild => {
            let history_item = history_item?;
            let action = match history_item.params.as_ref() {
                Some(params) => {
                    println!("{}:", t!("last-build-params").bold());
                    presets::print_params(params);
                    presets::select_last_build_action().await?
                }
                None => PresetBuildAction::Edit,
            };

            match action {
                PresetBuildAction::Build => Some((History::merge_parameters(history_item, &current_parameters), None)),
                PresetBuildAction::Edit => {
                    let parameter_definitions = History::apply_history_defaults(history_item, current_parameters);
                    let params = JenkinsClient::prompt_job_parameters(parameter_definitions).await?;
                    let post_action = presets::select_after_edit_action(false).await?;
                    handle_preset_save_action(presets, identity, "", params, post_action)
                }
                PresetBuildAction::EditAndUpdate => {
                    Some((History::merge_parameters(history_item, &current_parameters), None))
                }
                PresetBuildAction::EditAndSaveAs => {
                    let parameter_definitions = History::apply_history_defaults(history_item, current_parameters);
                    let params = JenkinsClient::prompt_job_parameters(parameter_definitions).await?;
                    handle_preset_save_action(presets, identity, "", params, PresetBuildAction::SaveAs)
                }
                PresetBuildAction::SaveAs => {
                    let params = History::merge_parameters(history_item, &current_parameters);
                    handle_preset_save_action(presets, identity, "", params, PresetBuildAction::SaveAs)
                }
                PresetBuildAction::Refill => {
                    let params = JenkinsClient::prompt_job_parameters(current_parameters).await?;
                    let post_action = presets::select_after_edit_action(false).await?;
                    handle_preset_save_action(presets, identity, "", params, post_action)
                }
                PresetBuildAction::Update => Some((History::merge_parameters(history_item, &current_parameters), None)),
            }
        }
        ParameterSource::JenkinsDefault => {
            let params = JenkinsClient::prompt_job_parameters(current_parameters).await?;
            let post_action = presets::select_after_edit_action(false).await?;
            handle_preset_save_action(presets, identity, "", params, post_action)
        }
        ParameterSource::ManagePresets => None,
    }
}

fn handle_preset_save_action(
    presets: &mut PresetStore,
    identity: &JobPresetIdentity,
    current_preset_name: &str,
    params: HashMap<String, jenkins::ParamInfo>,
    action: PresetBuildAction,
) -> Option<(HashMap<String, jenkins::ParamInfo>, Option<String>)> {
    match action {
        PresetBuildAction::Build
        | PresetBuildAction::Edit
        | PresetBuildAction::EditAndUpdate
        | PresetBuildAction::EditAndSaveAs
        | PresetBuildAction::Refill => Some((params, None)),
        PresetBuildAction::Update => {
            if current_preset_name.is_empty() {
                return Some((params, None));
            }
            if let Err(e) = presets.upsert_preset(identity, current_preset_name, params.clone()) {
                eprintln!("{}", t!("update-preset-failed", "error" => e.to_string()));
            } else {
                println!(
                    "{}",
                    t!("parameter-preset-updated", "name" => current_preset_name.to_string())
                );
            }
            Some((params, Some(current_preset_name.to_string())))
        }
        PresetBuildAction::SaveAs => {
            let preset_name = presets::prompt_preset_name(None)?;
            if presets.preset_exists(identity, &preset_name) {
                println!(
                    "{}",
                    t!("parameter-preset-overwrite", "name" => preset_name.clone()).yellow()
                );
            }
            if let Err(e) = presets.upsert_preset(identity, &preset_name, params.clone()) {
                eprintln!("{}", t!("update-preset-failed", "error" => e.to_string()));
                Some((params, None))
            } else {
                println!("{}", t!("parameter-preset-saved", "name" => preset_name.clone()));
                Some((params, Some(preset_name)))
            }
        }
    }
}

/// Main menu
async fn menu(service_step_enabled: bool, preset_arg: Option<&str>) -> bool {
    let config = CONFIG.lock().await;
    // println!("runtime_config:\n{:?}\n{:?}", config.global, config.jenkins);

    let global_config = config.global.clone();
    let jenkins_config = config
        .jenkins
        .as_ref()
        .expect("Jenkins configuration not found")
        .clone();
    drop(config);

    // Flow steps:
    // - Service selection (optional)
    // - Project selection (skipped if URL points to a job)
    // - Parameter selection
    let can_back_to_project = !jenkins_config.url.contains("/job/");
    let mut steps = StepTracker::new(service_step_enabled, can_back_to_project);
    let auth = if jenkins_config.user.is_empty() || jenkins_config.token.is_empty() {
        None
    } else {
        Some(format!("{}:{}", jenkins_config.user, jenkins_config.token))
    };
    let base_url = if jenkins_config.url.contains("/job/") {
        jenkins_config
            .url
            .split("/job/")
            .next()
            .unwrap_or(&jenkins_config.url)
            .to_string()
    } else {
        jenkins_config.url.clone()
    };
    if preset_arg.is_some() && !jenkins_config.url.contains("/job/") {
        eprintln!("{}", t!("preset-requires-job-url"));
        std::process::exit(1);
    }
    // let mut client = JenkinsClient::new(&config.url, &auth);
    let (event_sender, mut event_receiver) = mpsc::channel::<Event>(100);

    // Create client configuration
    let client_config = global_config.as_ref().map(|g| ClientConfig {
        timeout: g.timeout,
        follow_downstream: g.follow_downstream.unwrap_or(false),
    });

    let client = std::sync::Arc::new(tokio::sync::RwLock::new(JenkinsClient::new(
        &base_url,
        auth.as_deref(),
        if jenkins_config.cookie.is_empty() {
            None
        } else {
            Some(jenkins_config.cookie.as_str())
        },
        jenkins_config.cookie_refresh.clone(),
        client_config,
    )));
    // println!("config.url: {}", config.url); // client.read().await.base_url
    let mut history = History::new().unwrap();
    let mut presets = PresetStore::new().unwrap();
    let enable_history = jenkins_config.enable_history.unwrap_or(true);

    CTRL_C
        .set_ctx(std::sync::Arc::clone(&client), event_sender.clone())
        .await;

    // Main selection loop - allows going back from param selection to project selection
    let (job, job_url, user_params, preset_identity, used_preset_name) = loop {
        // Step 1: Select project
        steps.enter_project();
        let job = match get_project(&client, &jenkins_config, &mut history, &mut presets).await {
            Some(j) => j,
            None => {
                // Ctrl+C pressed
                if let Some(return_service) = handle_menu_back(&mut steps) {
                    return return_service;
                }
                continue;
            }
        };
        let relative_path = job.url.split("/job/").skip(1).collect::<Vec<&str>>().join("/job/");
        let job_url = format_url(&format!("{}/job/{}", base_url, relative_path));

        notify_if_update_available(); // before prompt params

        // Get build history
        let history_item = history.get_history(
            &HistoryEntry {
                name: job.name.clone(),
                job_url: job_url.clone(),
                ..Default::default()
            },
            &jenkins_config.url,
        );
        let preset_identity = JobPresetIdentity {
            service_url: base_url.clone(),
            job_url: job_url.clone(),
            job_name: job.name.clone(),
            display_name: Some(job.display_name.clone()),
        };

        // Get current Jenkins Job parameters
        let current_parameters = {
            let mut client_guard = client.write().await; // write for set job_url
            client_guard.job_url = Some(job_url.to_string());
            client_guard
                .get_job_parameters(&job_url)
                .await
                .expect(&t!("get-job-parameters-failed"))
        };

        // Select parameter source and build parameters
        steps.enter_params();
        let parameter_source = if let Some(preset_name) = preset_arg {
            match presets.find_preset(&preset_identity, preset_name) {
                Some(preset) => ParameterSource::Preset(preset),
                None => {
                    eprintln!("{}", t!("preset-not-found", "name" => preset_name.to_string()));
                    std::process::exit(1);
                }
            }
        } else {
            match presets::select_parameter_source(&presets, &preset_identity, history_item.is_some()).await {
                Some(source) => source,
                None => {
                    // Ctrl+C pressed
                    if let Some(return_service) = handle_menu_back(&mut steps) {
                        return return_service;
                    }
                    continue;
                }
            }
        };

        if matches!(parameter_source, ParameterSource::ManagePresets) {
            if presets::manage_presets(&mut presets, &preset_identity).await.is_none() {
                if let Some(return_service) = handle_menu_back(&mut steps) {
                    return return_service;
                }
            }
            continue;
        }

        let (user_params, used_preset_name) = if preset_arg.is_some() {
            match parameter_source {
                ParameterSource::Preset(preset) => (
                    presets::merge_preset_parameters(&preset, &current_parameters),
                    Some(preset.name),
                ),
                ParameterSource::ManagePresets => unreachable!("--preset always resolves to a preset source"),
                _ => unreachable!("--preset always resolves to a preset source"),
            }
        } else {
            match resolve_user_parameters(
                &mut presets,
                &preset_identity,
                parameter_source,
                history_item.as_ref(),
                current_parameters,
            )
            .await
            {
                Some(result) => result,
                None => {
                    if let Some(return_service) = handle_menu_back(&mut steps) {
                        return return_service;
                    }
                    continue;
                }
            }
        };

        // All selections completed
        break (job, job_url, user_params, preset_identity, used_preset_name);
    };

    // clear_screen();
    println!("Job URL: {}", job_url.underline().blue());
    // println!("user_params: {:?}", user_params);
    // std::process::exit(1); // debug params

    notify_if_update_available(); // before trigger build

    if enable_history {
        let mut history_param = HistoryEntry {
            job_url: job_url.clone(),
            name: job.name.clone(),
            display_name: Some(job.display_name.clone()),
            params: Some(user_params.clone()),
            created_at: Some(0),
            completed_at: Some(0),
        };
        if let Err(e) = history.upsert_history(&mut history_param) {
            eprintln!("{}", t!("update-history-failed", "error" => e.to_string()));
        }
    }

    if let Some(preset_name) = used_preset_name.as_deref() {
        if let Err(e) = presets.mark_preset_used(&preset_identity, preset_name) {
            eprintln!("{}", t!("update-preset-failed", "error" => e.to_string()));
        }
    }

    let queue_location = {
        let client_guard = client.read().await;
        match client_guard.trigger_build(&job_url, user_params).await {
            Ok(location) => location,
            Err(e) => {
                eprintln!("{}: {}", t!("trigger-build-failed"), e);
                std::process::exit(1);
            }
        }
    };

    CTRL_C.set_phase(CtrlCPhase::Polling);
    let build_url = {
        let client_guard = client.read().await;
        match client_guard.poll_queue_item(&queue_location, &mut event_receiver).await {
            Ok(url) => {
                CTRL_C.finish_polling();
                url
            }
            Err(e) => {
                CTRL_C.finish_polling();
                if e.to_string().contains("cancelled!") {
                    return false;
                }
                panic!("Failed to poll queue item: {}", e);
            }
        }
    };

    CTRL_C.set_phase(CtrlCPhase::Polling);
    let client_guard = client.read().await;
    match client_guard.poll_build_status(&build_url, &mut event_receiver).await {
        Ok(_) => {
            CTRL_C.finish_polling();
            // stop loop
            if enable_history {
                if let Err(e) = history.update_field(
                    &HistoryEntry {
                        name: job.name.clone(),
                        job_url: job_url.clone(),
                        ..Default::default()
                    },
                    |entry| {
                        entry.completed_at = Some(current_timestamp());
                    },
                ) {
                    eprintln!("Failed to update completed_at: {}", e);
                }
            }
        }
        Err(e) => {
            CTRL_C.finish_polling();
            if e.to_string().contains("cancelled!") {
                return false;
            }

            // // get full build log
            // flush_stdin();
            // let proceed: bool = dialoguer::Confirm::new()
            //     .with_prompt("Would you like to view the console log?")
            //     .default(true)
            //     .interact()
            //     .unwrap();
            // if proceed {
            //     if let Err(log_err) = client_guard.get_jenkins_console_log(&build_url).await {
            //     }
            // }
            println!(
                "Log URL: {}",
                format_url(&format!("{}/consoleText", build_url)).underline().blue(),
            );
        }
    }

    false
}

/// Get project information from URL or selection
async fn get_project(
    client: &std::sync::Arc<tokio::sync::RwLock<JenkinsClient>>,
    jenkins_config: &JenkinsConfig,
    history: &mut History,
    presets: &mut PresetStore,
) -> Option<jenkins::JenkinsJob> {
    if jenkins_config.url.contains("/job/") {
        match client.read().await.get_project(&jenkins_config.url).await {
            Ok(job) => Some(job),
            Err(e) => {
                eprintln!("{}: {}", t!("get-project-failed"), e);
                std::process::exit(1);
            }
        }
    } else {
        let projects: Vec<jenkins::JenkinsJob> = {
            let client_guard = client.read().await;
            match client_guard.get_projects().await {
                Ok(projects) => projects,
                Err(e) => {
                    eprintln!("{}: {}", t!("get-projects-failed"), e);
                    std::process::exit(1);
                }
            }
        };
        let mut projects = filter_projects(projects, jenkins_config);

        // Clean up obsolete history entries
        let project_names: Vec<String> = projects.iter().map(|p| p.name.clone()).collect();
        match history.cleanup_obsolete_projects(&project_names, &jenkins_config.url) {
            Ok(removed) => {
                if !removed.is_empty() {
                    println!(
                        "{}",
                        t!("history-cleanup", "count" => removed.len().to_string(), "names" => removed.join(", "))
                    );
                }
            }
            Err(e) => {
                eprintln!("{}", t!("history-cleanup-error", "error" => e.to_string()));
            }
        }

        match presets.cleanup_obsolete_projects(&project_names, &jenkins_config.url) {
            Ok(removed) => {
                if !removed.is_empty() {
                    println!(
                        "{}",
                        t!("preset-cleanup", "count" => removed.len().to_string(), "names" => removed.join(", "))
                    );
                }
            }
            Err(e) => {
                eprintln!("{}", t!("preset-cleanup-error", "error" => e.to_string()));
            }
        }

        // Get recent histories and reorder projects based on them
        let recent_histories = history.get_recent_histories(&jenkins_config.url, Some(5));

        // Promote recent projects to the front
        for history_entry in recent_histories.iter().rev() {
            if let Some(position) = projects.iter().position(|p| p.name == history_entry.name) {
                if position > 0 {
                    // Remove the project and insert it at the front
                    let project = projects.remove(position);
                    projects.insert(0, project);
                }
            }
        }
        // println!("latest build: {}, {:?}", latest_index, latest_history);

        // Select project
        let project_names: Vec<String> = projects
            .iter()
            .map(|p| format!("{} ({})", p.display_name, p.name))
            .collect();

        notify_if_update_available(); // before select project

        let selection =
            prompt::handle_selection_opt(prompt::with_prompt_kind(prompt::PromptKind::FuzzySelectVim, || {
                FuzzySelect::with_theme(&ColorfulTheme::default())
                    .with_prompt(t!("select-project-prompt"))
                    .items(&project_names)
                    .default(0)
                    // .report(false) // Display the selected project
                    .vim_mode(true) // Esc, j|k
                    .with_initial_text("")
                    .interact_opt()
            }));

        // Check if user pressed Ctrl+C
        match selection {
            Some(idx) => {
                let job = projects.get(idx).expect(&t!("select-project-failed"));
                Some(job.clone())
            }
            None => None, // Ctrl+C pressed - go back
        }
    }
}
