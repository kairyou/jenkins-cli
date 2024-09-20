use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use colored::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};

// use super::{JenkinsJob, JenkinsResponse, JenkinsJobConfig, JenkinsJobParameter};
use crate::i18n::macros::t;
use crate::{
    jenkins::{self, Event, JenkinsJob, JenkinsJobParameter, JenkinsResponse},
    spinner,
    utils::{clear_previous_line, clear_screen, delay, format_url, get_current_branch, get_git_branches},
};

/// Represents a Jenkins client.
pub struct JenkinsClient {
    pub base_url: String,
    authorization: String,
    client: reqwest::Client,
    // shared states
    pub job_url: Option<String>, // e.g. http://jenkins_url/job/job_name
}

impl JenkinsClient {
    /// Builds the headers for the request.
    ///
    /// # Arguments
    /// * `extra_headers` - Additional headers to include in the request.
    ///
    /// # Returns
    /// A `Result` containing the headers or an `anyhow::Error` if the headers cannot be built.
    fn build_headers(&self, extra_headers: Option<HashMap<String, String>>) -> Result<HeaderMap, anyhow::Error> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&self.authorization).map_err(|e| anyhow!(e.to_string()))?,
        );
        if let Some(extra) = extra_headers {
            for (key, value) in extra {
                headers.insert(
                    key.parse::<HeaderName>().map_err(|e| anyhow!(e.to_string()))?,
                    HeaderValue::from_str(&value).map_err(|e| anyhow!(e.to_string()))?,
                );
            }
        }
        Ok(headers)
    }

    /// Handles the response from the Jenkins server.
    ///
    /// # Arguments
    ///
    /// * `result` - The result of the response from the server.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or an `anyhow::Error` if the response is not successful.
    async fn handle_response(&self, result: Result<reqwest::Response, reqwest::Error>) -> Result<reqwest::Response> {
        match result {
            Ok(response) => {
                if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                    eprintln!("Error: Unauthorized (401). Please check your credentials.");
                    return Err(anyhow::anyhow!("Unauthorized (401): {}", response.status()));
                }
                if !response.status().is_success() {
                    // not 2xx
                    eprintln!("Error: HTTP request failed with status code {}", response.status());
                    return Err(anyhow::anyhow!("HTTP request failed: {}", response.status()));
                }
                Ok(response)
            }
            Err(e) => {
                if e.is_connect() {
                    eprintln!("Connection error: {:?}", e);
                } else if e.is_timeout() {
                    eprintln!("Request timed out: {:?}", e);
                } else if e.is_request() {
                    eprintln!("Request error: {:?}", e);
                } else {
                    eprintln!("Other error: {:?}", e);
                }
                Err(anyhow::anyhow!(e))
            }
        }
    }

    /// Creates a new instance of `JenkinsClient`.
    ///
    /// # Arguments
    ///
    /// * `base_url` - The base URL of the Jenkins server.
    /// * `authorization` - The authorization token for accessing the Jenkins server.
    ///
    /// # Returns
    ///
    /// A new instance of `JenkinsClient`.
    pub fn new(base_url: &str, authorization: &str) -> Self {
        let authorization = format!("Basic {}", STANDARD.encode(authorization));
        // println!("Authorization: {}", authorization);
        // std::env::set_var("NO_PROXY", "jenkins.example.com,other.example.com"); // Bypass proxy
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true) // Ignore SSL verification
            .no_proxy() // Ignore proxy to avoid potential DNS resolution failure
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("Jenkins CLI")
            .build()
            .expect("Failed to create reqwest client");
        // curl -k --noproxy '*' --user "uusername:token" "http://jenkins_url/api/json"
        Self {
            base_url: base_url.to_string(),
            authorization,
            client,
            job_url: None,
        }
    }

    /// Retrieves the list of projects from the Jenkins server.
    ///
    /// # Returns
    ///
    /// A `Result` containing a vector of `JenkinsJob` or an `anyhow::Error` if the request fails.
    pub async fn get_projects(&self) -> Result<Vec<JenkinsJob>, anyhow::Error> {
        let url = format_url(&format!(
            "{}/api/json?tree=jobs[name,displayName,url]&pretty=false",
            self.base_url
        ));
        let headers = self.build_headers(None)?;
        // println!("{}, {:?}", url, headers);
        let result = self.client.get(&url).headers(headers).send().await;
        // println!("{:?}", response);
        let response = self.handle_response(result).await?;
        let json_response: JenkinsResponse = response.json().await?;
        // println!("get_projects: {:?}", json_response);
        Ok(json_response.jobs)
    }

    /// Retrieves the parameters of a specific job from the Jenkins server.
    ///
    /// # Arguments
    ///
    /// * `job_url` - The URL of the job. e.g. http://jenkins_url/job/job_name
    ///
    /// # Returns
    ///
    /// A `Result` containing a vector of `JenkinsJobParameter` or an `anyhow::Error` if the request fails.
    pub async fn get_job_parameters(&mut self, job_url: &str) -> Result<Vec<JenkinsJobParameter>, anyhow::Error> {
        self.job_url = Some(job_url.to_string());
        // /api/json doesn't have trim information; get full configuration from /config.xml
        // @zh /api/json 无 trim 信息; 从 /config.xml 获取完整配置
        let url = format_url(&format!("{}/config.xml", job_url));
        let headers = self.build_headers(None)?;
        // println!("get_job: {}", url);
        let result = self.client.get(&url).headers(headers).send().await;
        let response = self.handle_response(result).await?;
        let xml_response = response.text().await?;
        // println!("xml_response: {:?}", xml_response);
        let parameters = jenkins::parse_jenkins_job_parameter(&xml_response);
        // println!("parameters: {:?}", parameters);
        Ok(parameters)
    }

    /// Prompts the user to enter values for the given parameter definitions.
    ///
    /// # Arguments
    ///
    /// * `parameter_definitions` - The parameter definitions.
    ///
    /// # Returns
    ///
    /// A `HashMap` containing the parameter names and their corresponding values.
    pub async fn prompt_job_parameters(parameter_definitions: Vec<JenkinsJobParameter>) -> HashMap<String, String> {
        use dialoguer::theme::ColorfulTheme; // ColorfulTheme/SimpleTheme
        let mut parameters = HashMap::new();
        let mut branches = get_git_branches();
        let branch_names = ["GIT_BRANCH", "gitBranch"];

        // for string, text, password
        fn prompt_user_input(fmt_name: &str, fmt_desc: &str, default_value: &str, trim: Option<bool>) -> String {
            let user_value: String = dialoguer::Input::with_theme(&ColorfulTheme::default())
                .with_prompt(format!("{}{}", t!("prompt-input", "name" => fmt_name), fmt_desc))
                .with_initial_text(default_value.to_string())
                .allow_empty(true)
                .interact_text()
                .unwrap_or_else(|_e| {
                    std::process::exit(0);
                });

            if trim.unwrap_or(false) {
                user_value.trim().to_string()
            } else {
                user_value
            }
        }

        for param in parameter_definitions {
            // println!("param: {:?}", param);
            let JenkinsJobParameter {
                param_type,
                name,
                description,
                default_value,
                choices,
                trim,
                ..
            } = param;
            let default_value = default_value.unwrap_or_else(|| "".to_string());
            let fmt_name = format!("'{}'", name.bold().yellow());
            let fmt_desc = description
                .as_ref()
                .map_or("".to_string(), |d| format!(" ({})", d.bold().blue()));
            // let fmt_choices = choices.as_ref().map_or("".to_string(), |c| {
            //     format!(" [可选值: {}]", c.join(", ").bold().green())
            // });
            let final_value = if let Some(choices) = choices {
                // Use Select to display the Choice list
                let selection = dialoguer::FuzzySelect::with_theme(&ColorfulTheme::default())
                    .with_prompt(format!("{}{}", t!("prompt-select", "name" => &fmt_name), fmt_desc))
                    .items(&choices)
                    .default(0)
                    .interact()
                    .unwrap_or_else(|_e| {
                        std::process::exit(0);
                    });

                choices[selection].clone()
            } else if param_type.as_deref() == Some("boolean") {
                let default_bool = default_value.parse::<bool>().unwrap_or(false);
                let value = dialoguer::Confirm::with_theme(&ColorfulTheme::default())
                    .with_prompt(format!("{}{}", t!("prompt-confirm", "name" => fmt_name), fmt_desc))
                    .default(default_bool)
                    .show_default(true)
                    .interact()
                    .unwrap_or_else(|_e| {
                        std::process::exit(0);
                    });
                value.to_string()
            } else if param_type.as_deref() == Some("password") {
                let input = dialoguer::Password::with_theme(&ColorfulTheme::default())
                    .with_prompt(format!("{}{}", t!("prompt-password", "name" => &fmt_name), fmt_desc))
                    .allow_empty_password(true)
                    .interact()
                    .unwrap_or_else(|_e| {
                        std::process::exit(0);
                    });
                if input.is_empty() {
                    default_value.to_string()
                } else {
                    input
                }
            } else if !branches.is_empty()
                && branch_names
                    .iter()
                    .any(|&b| name.to_lowercase().contains(&b.to_lowercase()))
            {
                // branches.retain(|branch| branch != &default_value); // Remove branch
                // If the parameter name contains GIT_BRANCH
                let current_branch = get_current_branch();
                // Add `manual input` option at the front
                let manual_input = t!("manual-input");
                branches.insert(0, manual_input.clone());
                // Move current_branch to the front
                if let Some(pos) = branches.iter().position(|b| b == &current_branch) {
                    branches.remove(pos);
                    branches.insert(1, current_branch.clone());
                }
                // Move default branch to the front
                if !default_value.is_empty() {
                    if let Some(pos) = branches.iter().position(|b| b == &default_value) {
                        branches.remove(pos);
                    }
                    branches.insert(1, default_value.clone());
                }

                // Priority: default_value, then current_branch, finally use 0
                let default_selection = branches
                    .iter()
                    .position(|b| b == &default_value)
                    .or_else(|| branches.iter().position(|b| b == &current_branch))
                    .unwrap_or(0);
                let custom_theme = ColorfulTheme {
                    // active_item_style: console::Style::new(), // Cancel default style
                    ..ColorfulTheme::default()
                };
                let selection = dialoguer::FuzzySelect::with_theme(&custom_theme)
                    .with_prompt(format!(
                        "{}{}",
                        t!("prompt-select-branch", "name" => &fmt_name),
                        fmt_desc
                    ))
                    .items(&branches)
                    .default(default_selection)
                    .vim_mode(true) // Esc, j|k
                    .with_initial_text("")
                    .interact()
                    .unwrap_or_else(|_e| {
                        std::process::exit(0);
                    });
                if branches[selection] == manual_input {
                    prompt_user_input(&fmt_name, &fmt_desc, "", trim)
                } else {
                    branches[selection].clone()
                }
            } else {
                // For other types, use text input
                prompt_user_input(&fmt_name, &fmt_desc, &default_value, trim)
            };

            let output_value = if param_type.as_deref() == Some("password") {
                "********".to_string()
            } else {
                final_value.clone()
            };
            println!("{}", output_value);
            parameters.insert(name, final_value);
        }
        parameters
    }

    /// Triggers a build for a specific job on the Jenkins server.
    ///
    /// # Arguments
    /// * `job_url` - The URL of the job.
    /// * `parameters` - The parameters to pass to the job.
    ///
    /// # Returns
    /// A `Result` containing the queue_location or an `anyhow::Error` if the request fails.
    pub async fn trigger_build(
        &self,
        job_url: &str,
        parameters: HashMap<String, String>,
    ) -> Result<String, anyhow::Error> {
        // Triggering with format!("{}/build?delay=0sec", job_url) doesn't use a queue
        let url = format_url(&format!("{}/buildWithParameters", job_url));
        let headers = self.build_headers(None)?;
        let result = self.client.post(&url).headers(headers).form(&parameters).send().await;
        let response = self.handle_response(result).await?;
        // println!("params: {:?}", parameters);
        // queue URL, e.g. http://jenkins_url/queue/item/1/
        let queue_location = response
            .headers()
            .get("Location")
            .ok_or_else(|| anyhow!("Missing Location header"))?
            .to_str()?;
        // println!("Queue location: {}", format_url(&format!("{}/api/json", queue_location)));
        Ok(queue_location.to_string())
    }

    /// Poll the queue item until it is executed and get the build URL
    /// e.g. http://jenkins_url/job/job_name/1/
    pub async fn poll_queue_item(
        &self,
        queue_url: &str,
        event_receiver: &mut mpsc::Receiver<Event>,
    ) -> Result<String, anyhow::Error> {
        let api_url = format_url(&format!("{}/api/json", queue_url));
        let stop_once = std::sync::Once::new();
        let spinner = spinner::Spinner::new(t!("polling-queue-item"));

        // detect Enter key press
        let should_exit = std::sync::Arc::new(AtomicBool::new(false));
        let detection_task = tokio::spawn({
            let should_exit = should_exit.clone();
            async move {
                check_for_enter_key(should_exit).await.unwrap();
            }
        });

        let result = loop {
            tokio::select! {
                _ = delay(2 * 1000) => {
                    let headers = self.build_headers(None)?;
                    let result = self.client.get(&api_url).headers(headers).send().await;
                    let response = self.handle_response(result).await?;
                    let queue_item: serde_json::Value = response.json().await?;
                    // println!("{}, queue: {:?}", api_url, queue_item);
                    if let Some(executable) = queue_item["executable"].as_object() {
                        // if let Some(build_url) = executable["url"].as_str() // maybe domain is different
                        if let Some(number) = executable["number"].as_i64() {
                            let job_url = self.job_url.as_ref().unwrap();
                            let build_url = format!("{}/{}", job_url, number);
                            stop_once.call_once(|| {
                                spinner.finish_with_message(format!("Build URL: {}", build_url.underline().blue()));
                            });
                            break Ok(build_url.to_string());
                        }
                    }
                },
                _ = event_receiver.recv() => {
                    // println!("{}", "poll_queue_item cancelled".red());
                    stop_once.call_once(|| {
                        spinner.finish_with_message("".to_string());
                    });
                    break Err(anyhow!("cancelled!"));
                },
            }
        };

        // Set exit flag and wait for the enter key detection task to complete
        should_exit.store(true, Ordering::SeqCst);
        detection_task.await.unwrap(); // Wait for task completion

        result
    }

    /// Poll the build status until it completes
    ///
    /// # Arguments
    /// * `build_url` - The URL of the build
    /// * `event_receiver` - A channel receiver for cancellation events
    ///
    /// # Returns
    /// * `Ok(())` if the build succeeds
    /// * `Err` with the build result if it fails
    /// * `Err` with "cancelled!" if the polling is cancelled
    pub async fn poll_build_status(
        &self,
        build_url: &str,
        event_receiver: &mut mpsc::Receiver<Event>,
    ) -> Result<(), anyhow::Error> {
        let api_url = format_url(&format!("{}/api/json", build_url));
        let spinner = spinner::Spinner::new("".to_string());
        let stop_once = std::sync::Once::new();
        let mut last_log_length = 0; // Initialize the length of the last read log
        loop {
            tokio::select! {
                _ = delay((1000.0 * 0.2) as u64) => {
                    let headers = self.build_headers(None)?;
                    let result = self.client.get(&api_url).headers(headers).send().await;
                    let response = self.handle_response(result).await?;
                    let build_info: serde_json::Value = response.json().await?;

                    // Retrieve and print the incremental part of Jenkins console log
                    match self.get_jenkins_progressive_text(build_url, last_log_length).await {
                        Ok((log, new_length)) => {
                            spinner.suspend(|| {
                                print!("{}", log);
                            });
                            last_log_length = new_length;
                        }
                        Err(e) => {
                            spinner.suspend(|| {
                              println!("Failed to retrieve console log: {}", e);
                            });
                        }
                    }

                    if build_info["building"].as_bool().unwrap_or(false) {
                        delay((1000.0 * 0.5) as u64).await;
                    } else {
                        let result = build_info["result"].as_str().unwrap_or("UNKNOWN"); // or inProgress
                        return if result == "SUCCESS" {
                            stop_once.call_once(|| {
                                spinner.finish_with_message(format!("Build result: {}", result.bold().green()));
                            });
                            Ok(())
                        } else {
                            stop_once.call_once(|| {
                                spinner.finish_with_message(format!("Build result: {}", result.bold().red()));
                            });
                            Err(anyhow!(result.red()))
                        };
                    }
                },
                _ = event_receiver.recv() => {
                    // println!("{}", "poll_build_status cancelled".red());
                    stop_once.call_once(|| {
                        spinner.finish_with_message("".to_string());
                    });
                    return Err(anyhow!("cancelled!"));
                },
                // _ = spawn_and_handle_enter_key() => {
                // },
            }
        }
    }

    /// Retrieves the incremental part of the Jenkins build log
    pub async fn get_jenkins_progressive_text(
        &self,
        build_url: &str,
        start: usize,
    ) -> Result<(String, usize), anyhow::Error> {
        let api_url = format_url(&format!("{}/logText/progressiveText?start={}", build_url, start));
        let headers = self.build_headers(None)?;
        let result = self.client.get(&api_url).headers(headers).send().await;
        let response = self.handle_response(result).await?;

        // Get the new length from the 'X-Text-Size' header
        let new_length = response
            .headers()
            .get("X-Text-Size")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(start);

        let console_log = response.text().await?;

        Ok((console_log, new_length))
    }

    /// Get Jenkins build log
    #[allow(dead_code)]
    pub async fn get_jenkins_console_log(&self, build_url: &str) -> Result<(), anyhow::Error> {
        let api_url = format_url(&format!("{}/consoleText", build_url));
        let headers = self.build_headers(None)?;
        let result = self.client.get(&api_url).headers(headers).send().await;
        let response = self.handle_response(result).await?;
        let console_log = response.text().await?;
        clear_screen();
        println!("{}", console_log);
        Ok(())
    }

    /// Check if there is an ongoing build and return the build status and number
    pub async fn is_building(&self) -> Result<(bool, Option<u32>), anyhow::Error> {
        let job_url = self.job_url.as_ref().unwrap();
        // println!("job_url: {:?}", job_url);
        let api_url = format_url(&format!("{}/lastBuild/api/json", job_url));
        let headers = self.build_headers(None)?;
        let result = self.client.get(&api_url).headers(headers).send().await;
        let response = self.handle_response(result).await?;
        let build_info: serde_json::Value = response.json().await?;
        // println!("build_info: {:?}", build_info);
        let is_building = build_info["building"].as_bool().unwrap_or(false);
        let build_number = build_info["number"].as_u64().map(|n| n as u32);
        Ok((is_building, build_number))
    }
    #[allow(dead_code)]
    pub async fn cancel_build(&self, build_number: Option<u32>) -> Result<(), anyhow::Error> {
        let api_url = match &self.job_url {
            Some(url) => match build_number {
                Some(number) => format_url(&format!("{}/{}/stop", url, number)),
                _ => format_url(&format!("{}/lastBuild/stop", url)),
            },
            _ => return Ok(()),
        };
        // println!("cancel_build: {:?}", api_url);
        let headers = self.build_headers(None)?;
        let result = self.client.post(&api_url).headers(headers).send().await;
        // self.handle_response(result).await?;
        match self.handle_response(result).await {
            Ok(_response) => {
                // println!("response: {:?}", _response);
                // println!("status: {:?}", _response.status()); // 302 redirect -> 200
                Ok(())
            }
            Err(e) => {
                println!("{}", t!("cancel-build-error", "error" => e.to_string()));
                Err(e)
            }
        }
    }
}

/// Prevent newline when Enter key is pressed
/// @zh 阻止回车换行. 显示 spinner 时回车, windows不会换行, linux会换行
#[doc(hidden)]
async fn check_for_enter_key(should_exit: std::sync::Arc<AtomicBool>) -> Result<(), anyhow::Error> {
    use crossterm::event::{self, Event, KeyCode};
    use std::time::Duration;
    let os = std::env::consts::OS;
    while !should_exit.load(Ordering::Relaxed) {
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key_event) = event::read()? {
                if key_event.code == KeyCode::Enter && os != "windows" {
                    clear_previous_line(); // Clear the previous line
                }
            }
        }
    }
    Ok(())
}
