use chrono::{DateTime, Local};
use clap::{Arg, Command};
use colored::*;
use dialoguer::{
    theme::ColorfulTheme,
    FuzzySelect,
    // MultiSelect,
    // Select,
};
use once_cell::sync::Lazy;
use regex::Regex;
use std::sync::atomic;
use tokio::{signal, sync::mpsc, sync::Mutex};

mod config;
mod env_checks;
mod i18n;
mod jenkins;
mod models;
mod spinner;
mod utils;

// use crate::i18n::I18n;
use crate::{
    config::{initialize_config, CONFIG},
    jenkins::{
        client::JenkinsClient,
        history::{History, HistoryItem},
        Event,
    },
    models::JenkinsConfig,
    utils::{clear_screen, current_timestamp, delay, flush_stdin, format_url},
};

// Flag indicating whether Ctrl+C has been pressed
static CTRL_C_PRESSED: atomic::AtomicBool = atomic::AtomicBool::new(false);
// Flag indicating whether the Ctrl+C handling logic has been completed
static CTRL_C_HANDLED: Lazy<atomic::AtomicBool> = Lazy::new(|| atomic::AtomicBool::new(false));
// Flag indicating whether a build is currently in progress
static LOADING: Lazy<std::sync::Arc<Mutex<bool>>> =
    Lazy::new(|| std::sync::Arc::new(Mutex::new(false)));

#[tokio::main]
async fn main() {
    // I18n::set_locale("zh-CN");
    initialize_config().await.unwrap();

    let matches = Command::new("jenkins")
        .version("1.0")
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
        .get_matches();
    // check_unsupported_terminal();
    clear_screen();
    {
        let mut config = CONFIG.lock().await;
        if let Some(url) = matches.get_one::<String>("url") {
            config.url = url.to_string();
        }
        if let Some(user) = matches.get_one::<String>("user") {
            config.user = user.to_string();
        }
        if let Some(token) = matches.get_one::<String>("token") {
            config.token = token.to_string();
        }
    }

    // if let Some(config) = matches.get_one::<String>("config") {
    //     println!("Config file used: {}", config);
    // }

    // main logic
    menu().await;

    // wait for Ctrl+C loop to finish
    while !CTRL_C_HANDLED.load(atomic::Ordering::SeqCst) {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}

// actions

fn filter_projects(
    projects: Vec<jenkins::JenkinsJob>,
    config: &JenkinsConfig,
) -> Vec<jenkins::JenkinsJob> {
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

    let includes = compile_patterns(config.includes.as_ref());
    let excludes = compile_patterns(config.excludes.as_ref());

    projects
        .into_iter()
        .filter(|project| {
            let display_name = &project.display_name;
            let name = &project.name;

            let matches_include = includes.is_empty()
                || includes
                    .iter()
                    .any(|re| re.is_match(display_name) || re.is_match(name));
            let matches_exclude = excludes
                .iter()
                .any(|re| re.is_match(display_name) || re.is_match(name));
            matches_include && !matches_exclude
        })
        .collect()
}

/// Main menu
async fn menu() {
    let config = CONFIG.lock().await;
    // println!("{}, {}, {}", config.url, config.user, config.token);
    let auth = format!("{}:{}", config.user, config.token);
    // let mut client = JenkinsClient::new(&config.url, &auth);
    let (event_sender, mut event_receiver) = mpsc::channel::<Event>(100);
    let client = std::sync::Arc::new(tokio::sync::RwLock::new(JenkinsClient::new(
        &config.url,
        &auth,
    )));
    // println!("config.url: {}", config.url); // client.read().await.base_url
    let mut history = History::new().unwrap();

    // Spawn a task to listen for Ctrl+C
    let _ctrl_c_handler = {
        let client_clone = std::sync::Arc::clone(&client);
        tokio::spawn(async move {
            handle_ctrl_c(client_clone, event_sender).await;
        })
    };

    let projects: Vec<jenkins::JenkinsJob> = {
        let client_guard = client.read().await;
        match client_guard.get_projects().await {
            Ok(projects) => projects,
            Err(_e) => {
                eprintln!("{}", t!("get-project-failed"));
                return;
            }
        }
    };
    let mut projects = filter_projects(projects, &config);
    // projects.iter().for_each(|project| println!("Name: {} ({})", project.display_name, project.name));

    let latest_history = history.get_latest_history(Some(&config.url));
    let latest_index: usize = latest_history
        .and_then(|entry| {
            Some(entry.name.as_str()) // String => &str
                    .and_then(|entry_name| projects.iter().position(|p| p.name == entry_name))
        })
        .unwrap_or(0);
    if latest_index != 0 && latest_index < projects.len() {
        let latest_project = projects.remove(latest_index);
        projects.insert(0, latest_project);
    }
    // println!("latest build: {}, {:?}", latest_index, latest_history);

    // Select project
    let project_names: Vec<String> = projects
        .iter()
        .map(|p| format!("{} ({})", p.display_name, p.name))
        .collect();

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt(t!("select-project-prompt"))
        .items(&project_names)
        .default(0)
        // .report(false) // Display the selected project
        .vim_mode(true) // Esc, j|k
        .with_initial_text("")
        .interact()
        .unwrap_or_else(|e| {
            if e.to_string().contains("interrupted") {
                std::process::exit(0);
            }
            eprintln!("{}: {}", t!("select-project-failed"), e);
            std::process::exit(1);
        });
    // Get project parameters
    let job = projects.get(selection).expect(&t!("get-project-failed"));

    // println!("Selected project: {}", job.display_name.cyan().bold());
    let job_url = format!("{}/job/{}", config.url, job.name);
    // println!("{}", job_url.underline().blue());

    // Get build history
    let history_item = history.get_history(
        &HistoryItem {
            name: job.name.clone(),
            job_url: job_url.clone(),
            ..Default::default()
        },
        Some(&config.url),
    );
    // Use last build params
    let use_previous_params = history_item.as_ref().map_or(false, |history| {
        let params = history.user_params.as_ref().unwrap();
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

        // println!("{:?}", params);
        // println!("{}", serde_json::to_string_pretty(&serde_json::json!(params)).unwrap());
        for (key, value) in params.iter() {
            println!(" {}: {}", key.yellow(), value);
        }
        dialoguer::Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(&t!("use-last-build-params"))
            .default(true)
            .interact()
            .unwrap_or_else(|_e| {
                std::process::exit(0);
            })
    });

    let user_params = if use_previous_params {
        // set self.job_url
        let mut client_guard = client.write().await;
        client_guard.job_url = Some(job_url.to_string());
        history_item.unwrap().user_params.unwrap_or_default()
    } else {
        let params = {
            let mut client_guard = client.write().await; // write for set job_url
            client_guard
                .get_job_parameters(&job_url)
                .await
                .expect(&t!("get-job-parameters-failed"))
        };
        JenkinsClient::prompt_job_parameters(params).await
    };

    // clear_screen();
    println!("Job URL: {}", job_url.underline().blue());
    // println!("user_params: {:?}", user_params);
    // std::process::exit(1); // debug params

    let mut history_param = HistoryItem {
        job_url: job_url.clone(),
        name: job.name.clone(),
        display_name: Some(job.display_name.clone()),
        user_params: Some(user_params.clone()),
        created_at: Some(0),
        completed_at: Some(0),
    };
    if let Err(e) = history.upsert_history(&mut history_param) {
        eprintln!("{}", t!("update-history-failed", "error" => e.to_string()));
    }

    let queue_location = {
        let client_guard = client.read().await;
        client_guard
            .trigger_build(&job_url, user_params)
            .await
            .expect(&t!("trigger-build-failed"))
    };

    *LOADING.lock().await = true;
    let build_url = {
        let client_guard = client.read().await;
        match client_guard
            .poll_queue_item(&queue_location, &mut event_receiver)
            .await
        {
            Ok(url) => {
                *LOADING.lock().await = false;
                url
            }
            Err(e) => {
                // println!("poll_queue_item: {}", e.to_string().red());
                *LOADING.lock().await = false;
                if e.to_string().contains("cancelled!") {
                    return;
                }
                panic!("Failed to poll queue item: {}", e);
            }
        }
    };
    // println!("Build URL: {}", build_url.underline().blue());

    *LOADING.lock().await = true;
    let client_guard = client.read().await;
    match client_guard
        .poll_build_status(&build_url, &mut event_receiver)
        .await
    {
        Ok(_) => {
            // println!("{}", "Build completed successfully.".green());
            *LOADING.lock().await = false;
            // stop loop
            CTRL_C_HANDLED.store(true, atomic::Ordering::SeqCst);
            if let Err(e) = history.update_field(
                &HistoryItem {
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
        Err(e) => {
            // println!("poll_build_status: {}", e.to_string().red());
            *LOADING.lock().await = false;
            if e.to_string().contains("cancelled!") {
                return;
            }
            CTRL_C_HANDLED.store(true, atomic::Ordering::SeqCst);

            // // get full build log
            // flush_stdin();
            // let proceed: bool = dialoguer::Confirm::new()
            //     .with_prompt("Would you like to view the console log?")
            //     .default(true)
            //     .interact()
            //     .unwrap();
            // if proceed {
            //     if let Err(log_err) = client_guard.get_jenkins_console_log(&build_url).await {
            //         println!("Failed to retrieve console log: {}", log_err);
            //     }
            // }
            println!(
                "Log URL: {}",
                format_url(&format!("{}/consoleText", build_url))
                    .underline()
                    .blue(),
            );
        }
    }
}

/// Handle Ctrl+C
async fn handle_ctrl_c(
    client: std::sync::Arc<tokio::sync::RwLock<JenkinsClient>>,
    event_sender: mpsc::Sender<Event>,
) {
    // Listen for Ctrl+C in a separate task, used to exit immediately when Ctrl+C is triggered multiple times
    // @zh 独立的任务监听 Ctrl+C, 用于多次触发 Ctrl+C 时立即退出
    tokio::spawn({
        async move {
            loop {
                signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
                if CTRL_C_PRESSED.load(atomic::Ordering::SeqCst) {
                    println!("Ctrl+C pressed again, exiting immediately.");
                    std::process::exit(1);
                }
                CTRL_C_PRESSED.store(true, atomic::Ordering::SeqCst);
                event_sender.send(Event::StopSpinner).await.unwrap();
            }
        }
    });
    signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
    // println!("Ctrl+C pressed");
    println!("Checking for running builds...");
    // println!("Loading {:?}", *LOADING.lock().await);

    if *LOADING.lock().await {
        flush_stdin();
        // Wait for the spinner to stop to prevent it from obscuring the prompt
        // @zh 等待 spinner 停止，防止 spinner 遮挡询问文字
        while *LOADING.lock().await {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        let prompt = t!("cancel-build-prompt").red().bold().to_string();
        let confirm = match dialoguer::Confirm::new()
            .with_prompt(prompt)
            .default(false)
            .interact()
        {
            Ok(result) => result,
            Err(_e) => {
                // eprintln!("Error reading input: {}", _e);
                return;
            }
        };
        if confirm {
            println!("{}", t!("cancelling-build").yellow());
            tokio::select! {
              _ = signal::ctrl_c() => {
                  // println!("Ctrl+C pressed again, exiting immediately.");
                  // std::process::exit(1);
              },
              _ = async {
                loop {
                  let client_guard = client.read().await;
                  if let Ok((building, id)) = client_guard.is_building().await {
                      // println!("!! : {}, id: {:?}", building, id);
                      if building {
                          if let Some(num) = id {
                              println!("{}: {}", t!("current-build-id"), num.to_string().cyan().bold());
                          }
                          match client_guard.cancel_build(id).await {
                              Ok(_) => {
                                  // Sometimes Jenkins continues building after cancellation, so we try multiple times
                                  let mut stopped = false;
                                  let max_attempts = 10;
                                  let mut attempts = 0;
                                  while !stopped && attempts < max_attempts {
                                      // println!("!! {}", format!("{}", attempts + 1).yellow());
                                      let (building, _) =
                                          client_guard.is_building().await.unwrap();
                                      if !building {
                                          stopped = true;
                                      } else {
                                          client_guard.cancel_build(id).await.unwrap(); // try again
                                          attempts += 1;
                                          delay(3 * 1000).await;
                                      }
                                  }
                                  if stopped {
                                      println!("{}", t!("build-cancelled").green());
                                  } else {
                                      eprintln!("{}", t!("cancel-build-failed").red());
                                  }
                                  break;
                              }
                              Err(e) => {
                                  eprintln!("{}: {}", t!("cancel-build-failed"), e.to_string().red());
                                  break;
                              }
                          }
                      }
                  } else {
                      eprintln!("{}", t!("check-build-status-failed").red());
                      // break;
                  }
                  delay(1000).await;
                }
              } => {
                // Build cancellation completed
              }
            }
        }
    }
    println!("{}", t!("bye"));
    CTRL_C_HANDLED.store(true, atomic::Ordering::SeqCst);
    std::process::exit(0);
}
