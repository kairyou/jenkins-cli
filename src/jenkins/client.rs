use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use colored::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

use regex::Regex;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE, COOKIE},
    StatusCode,
};
use serde_json::Value as JsonValue;

// use super::{JenkinsJob, JenkinsResponse, JenkinsJobConfig, JenkinsJobParameter};
use super::console_html::{self, DownstreamJobLink};
use crate::constants::{
    ParamType, DEFAULT_PARAM_VALUE, JENKINS_AUTO_BUILD_TYPES, JENKINS_BUILDABLE_TYPES, JENKINS_FOLDER_TYPE,
};
use crate::i18n::macros::t;
use crate::prompt;
use crate::{
    jenkins::{self, cookie::CookieStore, Event, JenkinsJob, JenkinsJobParameter, JenkinsResponse, ParamInfo},
    models::CookieRefreshConfig,
    spinner, terminal,
    utils::{
        clear_screen, delay, finish_terminal_line, format_url, get_current_branch, get_git_branches,
        reset_terminal_line,
    },
};

/// Configuration for the Jenkins client.
#[derive(Debug, Clone, Default)]
pub struct ClientConfig {
    /// HTTP request timeout in seconds (default: 30).
    pub timeout: Option<u64>,
    /// Follow detected downstream builds.
    pub follow_downstream: bool,
    // example:
    // pub max_retries: Option<u32>,
    // pub proxy: Option<String>,
    // pub verify_ssl: Option<bool>,
}

pub struct BuildStatus {
    pub building: bool,
    pub id: Option<u32>,
    pub last_build: Option<u32>,
    pub last_completed: Option<u32>,
    pub in_queue: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DownstreamBuild {
    pub job: DownstreamJobLink,
    pub build_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildIdentity {
    number: u64,
    upstream_url: String,
}

#[doc(hidden)]
pub struct BranchOptionsInput<'a> {
    pub branches: &'a [String],
    pub default_branch: Option<&'a str>,
    pub current_branch: Option<&'a str>,
    pub manual_input: &'a str,
}

/// Represents a Jenkins client.
pub struct JenkinsClient {
    pub base_url: String,
    authorization: Option<String>,
    cookie_store: CookieStore,
    cookie_refresh: Option<CookieRefreshConfig>,
    cookie_refresh_attempted: AtomicBool,
    client: reqwest::Client,
    follow_downstream: bool,
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
    fn build_headers(
        &self,
        include_auth: bool,
        extra_headers: Option<HashMap<String, String>>,
    ) -> Result<HeaderMap, anyhow::Error> {
        let mut headers = HeaderMap::new();
        // client.basic_auth(self.username.clone(), Some(self.token.clone()))
        if include_auth {
            if let Some(authorization) = self.authorization.as_ref() {
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(authorization).map_err(|e| anyhow!(e.to_string()))?,
                );
            }
        }
        if let Some(cookie) = self.cookie_store.header_value() {
            headers.insert(
                COOKIE,
                HeaderValue::from_str(&cookie).map_err(|e| anyhow!(e.to_string()))?,
            );
        }
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
                let status = response.status();
                self.cookie_store.update_from_response(&response, &self.base_url);
                if !status.is_success() {
                    let url = response.url().to_string();
                    let error_message = match status {
                        reqwest::StatusCode::UNAUTHORIZED => "Unauthorized (401): Please check your credentials.",
                        reqwest::StatusCode::FORBIDDEN => "Forbidden (403): You may not have sufficient permissions.",
                        reqwest::StatusCode::NOT_FOUND => "Not Found (404): The requested resource does not exist.",
                        _ => "Request failed",
                    };

                    eprintln!("Error: {}", error_message.red());
                    eprintln!("URL: {}", url);
                    // eprintln!("Response headers: {:?}", response.headers().clone());
                    // eprintln!("Response body: {}", response.text().await?);

                    return Err(anyhow::anyhow!("{} Status code: {}", error_message, status));
                }
                Ok(response)
            }
            Err(e) => {
                let base_msg = format!("{:?}", e).replace("reqwest::Error ", "");
                let error_msg = if let Some(source) = std::error::Error::source(&e) {
                    let source_msg = source
                        .source()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| source.to_string());
                    base_msg.replace(&format!("{:?}", source), &source_msg)
                } else {
                    base_msg
                };

                // eprintln!("Error {:?}", e);
                if e.is_connect() {
                    eprintln!("Connection error: {}", error_msg);
                } else if e.is_timeout() {
                    eprintln!("Request timed out: {}", error_msg);
                } else if e.is_request() {
                    eprintln!("Request error: {}", error_msg);
                } else {
                    eprintln!("Other error: {}", error_msg);
                }
                Err(anyhow::anyhow!(e))
            }
        }
    }

    // HTTP helpers (crumb + refresh-aware GET/POST).
    /// Try to fetch Jenkins crumb info. Returns None if crumb is not available.
    async fn try_get_crumb(&self) -> Result<Option<(String, String)>, anyhow::Error> {
        let url = format_url(&format!("{}/crumbIssuer/api/json", self.base_url));
        let headers = self.build_headers(true, None)?;
        let result = self.client.get(&url).headers(headers).send().await;
        let response = match result {
            Ok(response) => response,
            Err(e) => return Err(anyhow!(e)),
        };
        self.cookie_store.update_from_response(&response, &self.base_url);
        if !response.status().is_success() {
            return Ok(None);
        }
        let payload: serde_json::Value = response.json().await?;
        let field = payload["crumbRequestField"].as_str();
        let crumb = payload["crumb"].as_str();
        match (field, crumb) {
            (Some(field), Some(crumb)) => Ok(Some((field.to_string(), crumb.to_string()))),
            _ => Ok(None),
        }
    }

    async fn post_with_crumb_retry(
        &self,
        url: &str,
        form: Option<&HashMap<String, String>>,
    ) -> Result<reqwest::Response, anyhow::Error> {
        // CSRF retry: attempt to fetch Jenkins crumb on 403 and retry once.
        self.ensure_cookie_refresh_once().await?;
        let headers = self.build_headers(true, None)?;
        let builder = self.client.post(url).headers(headers);
        let builder = if let Some(form) = form {
            builder.form(form)
        } else {
            builder
        };
        let result = builder.send().await;

        match result {
            Ok(response) if response.status() == StatusCode::UNAUTHORIZED => {
                if self.refresh_cookie().await? {
                    let headers = self.build_headers(true, None)?;
                    let builder = self.client.post(url).headers(headers);
                    let builder = if let Some(form) = form {
                        builder.form(form)
                    } else {
                        builder
                    };
                    let retry = builder.send().await;
                    return self.handle_response(retry).await;
                }
                self.handle_response(Ok(response)).await
            }
            Ok(response) if response.status() == StatusCode::FORBIDDEN => {
                self.cookie_store.update_from_response(&response, &self.base_url);
                if let Some((field, crumb)) = self.try_get_crumb().await? {
                    let mut extra = HashMap::new();
                    extra.insert(field.clone(), crumb.clone());
                    let headers = self.build_headers(true, Some(extra))?;
                    let builder = self.client.post(url).headers(headers);
                    let builder = if let Some(form) = form {
                        builder.form(form)
                    } else {
                        builder
                    };
                    let retry = builder.send().await;
                    if let Ok(retry_response) = &retry {
                        if (retry_response.status() == StatusCode::UNAUTHORIZED
                            || retry_response.status() == StatusCode::FORBIDDEN)
                            && self.refresh_cookie().await?
                        {
                            let mut extra = HashMap::new();
                            extra.insert(field, crumb);
                            let headers = self.build_headers(true, Some(extra))?;
                            let builder = self.client.post(url).headers(headers);
                            let builder = if let Some(form) = form {
                                builder.form(form)
                            } else {
                                builder
                            };
                            let retry2 = builder.send().await;
                            return self.handle_response(retry2).await;
                        }
                    }
                    self.handle_response(retry).await
                } else {
                    if self.refresh_cookie().await? {
                        let headers = self.build_headers(true, None)?;
                        let builder = self.client.post(url).headers(headers);
                        let builder = if let Some(form) = form {
                            builder.form(form)
                        } else {
                            builder
                        };
                        let retry = builder.send().await;
                        return self.handle_response(retry).await;
                    }
                    self.handle_response(Ok(response)).await
                }
            }
            _ => self.handle_response(result).await,
        }
    }

    // GET once (optionally refresh cookie on 401/403), without handle_response.
    async fn get_with_refresh_raw(&self, url: &str) -> Result<reqwest::Response, anyhow::Error> {
        self.ensure_cookie_refresh_once().await?;
        let headers = self.build_headers(true, None)?;
        let response = self.client.get(url).headers(headers).send().await?;
        if (response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN)
            && self.refresh_cookie().await?
        {
            let headers = self.build_headers(true, None)?;
            let retry = self.client.get(url).headers(headers).send().await?;
            return Ok(retry);
        }
        Ok(response)
    }

    // GET with refresh + standard error handling.
    async fn get_with_refresh(&self, url: &str) -> Result<reqwest::Response, anyhow::Error> {
        let response = self.get_with_refresh_raw(url).await?;
        self.handle_response(Ok(response)).await
    }

    // Cookie refresh helpers.
    // Best-effort refresh once per client to avoid stale cookies before first API call.
    async fn ensure_cookie_refresh_once(&self) -> Result<(), anyhow::Error> {
        if self.cookie_refresh.is_none() {
            return Ok(());
        }
        if crate::utils::debug_enabled() {
            crate::utils::debug_line(&format!(
                "[debug] cookie_refresh: attempting (already_attempted={}, has_cookie={})",
                self.cookie_refresh_attempted.load(Ordering::SeqCst),
                self.cookie_store.header_value().is_some()
            ));
        }
        if self.cookie_refresh_attempted.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let has_cookie = self.cookie_store.header_value().is_some();
        if let Err(e) = self.refresh_cookie().await {
            if crate::utils::debug_enabled() {
                crate::utils::debug_line(&format!("[debug] cookie_refresh: failed: {}", e));
            }
            if !has_cookie {
                return Err(e);
            }
        }
        Ok(())
    }

    // Perform refresh request and update cookies from response.
    async fn refresh_cookie(&self) -> Result<bool, anyhow::Error> {
        let config = match self.cookie_refresh.as_ref() {
            Some(config) => config,
            None => return Ok(false),
        };
        if config.url.is_empty() {
            return Ok(false);
        }

        let method = if config.method.is_empty() {
            "POST"
        } else {
            config.method.as_str()
        };
        // Resolve template variables in request params (e.g. ${cookie.jwt_token}).
        let query = self.resolve_params(&config.request.query)?;
        let form = self.resolve_params(&config.request.form)?;
        let json = self.resolve_json_value(&config.request.json)?;
        let has_json_body = !json.is_null();
        if !form.is_empty() && has_json_body {
            return Err(anyhow!("cookie_refresh.request cannot include both form and json"));
        }
        if method.eq_ignore_ascii_case("GET") && (!form.is_empty() || has_json_body) {
            return Err(anyhow!("cookie_refresh.request body is not allowed for GET"));
        }

        let extra_headers = self.resolve_params(&config.request.headers)?;
        let mut headers = self.build_headers(false, Some(extra_headers))?;
        let resolved_url = self.resolve_template(&config.url)?;
        if crate::utils::debug_enabled() {
            let mut debug_url = resolved_url.clone();
            if let Ok(mut parsed) = reqwest::Url::parse(&resolved_url) {
                for (key, value) in &query {
                    parsed.query_pairs_mut().append_pair(key, value);
                }
                debug_url = parsed.to_string();
            }
            crate::utils::debug_line(&format!("[debug] cookie_refresh: {} {}", method, debug_url));
            if let Some(value) = headers.get(COOKIE).and_then(|v| v.to_str().ok()) {
                crate::utils::debug_line(&format!("[debug] cookie_refresh: request_header_cookie={}", value));
            }
            if !query.is_empty() || !form.is_empty() || has_json_body {
                crate::utils::debug_line(&format!(
                    "[debug] cookie_refresh: params query={:?} form={:?} json={:?}",
                    query, form, json
                ));
            }
        }
        if has_json_body && !headers.contains_key(CONTENT_TYPE) {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }
        let mut request = self
            .client
            .request(method.parse::<reqwest::Method>()?, &resolved_url)
            .headers(headers);
        if !query.is_empty() {
            request = request.query(&query);
        }
        if has_json_body {
            request = request.json(&json);
        } else if !form.is_empty() {
            request = request.form(&form);
        }

        let response = self.handle_response(request.send().await).await?;
        // Apply extracted cookies; if empty, rely on Set-Cookie headers instead.
        if !config.cookie_updates.is_empty() {
            let updates = self.extract_cookie_updates(response, &config.cookie_updates).await?;
            self.cookie_store.update_from_pairs(updates, &self.base_url);
        }
        Ok(true)
    }

    // Replace ${cookie.<name>} with current cookie values.
    fn resolve_template(&self, input: &str) -> Result<String> {
        let mut output = String::new();
        let mut rest = input;
        while let Some(start) = rest.find("${cookie.") {
            output.push_str(&rest[..start]);
            let after = &rest[start + 9..];
            let end = after.find('}').ok_or_else(|| anyhow!("Invalid template: {}", input))?;
            let key = &after[..end];
            let value = self
                .cookie_store
                .get_value(key)
                .ok_or_else(|| anyhow!("Missing cookie value: {}", key))?;
            output.push_str(&value);
            rest = &after[end + 1..];
        }
        output.push_str(rest);
        Ok(output)
    }

    fn resolve_params(&self, params: &HashMap<String, String>) -> Result<HashMap<String, String>> {
        let mut resolved = HashMap::new();
        for (key, value) in params {
            resolved.insert(key.clone(), self.resolve_template(value)?);
        }
        Ok(resolved)
    }

    fn resolve_json_value(&self, value: &JsonValue) -> Result<JsonValue> {
        match value {
            JsonValue::String(text) => Ok(JsonValue::String(self.resolve_template(text)?)),
            JsonValue::Array(items) => {
                let mut resolved = Vec::with_capacity(items.len());
                for item in items {
                    resolved.push(self.resolve_json_value(item)?);
                }
                Ok(JsonValue::Array(resolved))
            }
            JsonValue::Object(map) => {
                let mut resolved = serde_json::Map::with_capacity(map.len());
                for (key, item) in map {
                    resolved.insert(key.clone(), self.resolve_json_value(item)?);
                }
                Ok(JsonValue::Object(resolved))
            }
            _ => Ok(value.clone()),
        }
    }

    // Extract cookie updates from response by spec (body.json / body.regex / header).
    async fn extract_cookie_updates(
        &self,
        response: reqwest::Response,
        specs: &HashMap<String, String>,
    ) -> Result<Vec<(String, String)>> {
        let headers = response.headers().clone();
        let body = response.text().await.unwrap_or_default();
        let mut json: Option<serde_json::Value> = None;

        let mut updates = Vec::new();
        for (cookie_name, spec) in specs {
            let token = Self::extract_token_value(&headers, &body, &mut json, spec)?;
            if crate::utils::debug_enabled() {
                eprintln!(
                    "[debug] cookie_refresh: extracted {} (len={}) from {}",
                    cookie_name,
                    token.len(),
                    spec
                );
            }
            updates.push((cookie_name.to_string(), token));
        }
        Ok(updates)
    }

    // Parse a single value from response based on the spec.
    fn extract_token_value(
        headers: &reqwest::header::HeaderMap,
        body: &str,
        json: &mut Option<serde_json::Value>,
        spec: &str,
    ) -> Result<String> {
        let (kind, value) = spec
            .split_once(':')
            .ok_or_else(|| anyhow!("Invalid cookie_updates spec: {}", spec))?;
        match kind {
            "body.json" => {
                if json.is_none() {
                    *json = Some(serde_json::from_str(body).map_err(|e| {
                        anyhow!("Response is not valid JSON: {}. Body: {}", e, Self::truncate_body(body))
                    })?);
                }
                let payload = json.as_ref().unwrap();
                let token_path = value;
                if let Some(token) = Self::get_json_path(payload, token_path).and_then(|value| value.as_str()) {
                    return Ok(token.to_string());
                }
                Err(anyhow!(
                    "Missing token at path: {}. Body: {}",
                    token_path,
                    Self::truncate_body(body)
                ))
            }
            "header" => headers
                .get(value)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_string())
                .ok_or_else(|| anyhow!("Missing token header: {}", value)),
            "body.regex" => {
                let re = Regex::new(value)?;
                let caps = re.captures(body).ok_or_else(|| anyhow!("Token regex not matched"))?;
                caps.get(1)
                    .map(|m| m.as_str().to_string())
                    .ok_or_else(|| anyhow!("Token regex missing capture group"))
            }
            _ => Err(anyhow!("Unsupported cookie_updates spec: {}", spec)),
        }
    }

    fn truncate_body(body: &str) -> String {
        let limit = 1200;
        if body.len() <= limit {
            body.to_string()
        } else {
            format!("{}...[truncated]", &body[..limit])
        }
    }

    fn get_json_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
        let mut current = value;
        for part in path.split('.') {
            if part.is_empty() {
                continue;
            }
            current = current.get(part)?;
        }
        Some(current)
    }

    /// Creates a new instance of `JenkinsClient`.
    ///
    /// # Arguments
    ///
    /// * `base_url` - The base URL of the Jenkins server.
    /// * `authorization` - The authorization token for accessing the Jenkins server.
    /// * `config` - Client configuration options.
    ///
    /// # Returns
    ///
    /// A new instance of `JenkinsClient`.
    pub fn new(
        base_url: &str,
        authorization: Option<&str>,
        cookie: Option<&str>,
        cookie_refresh: Option<CookieRefreshConfig>,
        config: Option<ClientConfig>,
    ) -> Self {
        let authorization = authorization.map(|value| format!("Basic {}", STANDARD.encode(value)));
        let persist_keys_hint = cookie_refresh.as_ref().and_then(|config| {
            if config.cookie_updates.is_empty() {
                None
            } else {
                Some(config.cookie_updates.keys().cloned().collect::<HashSet<String>>())
            }
        });
        let cookie_store = CookieStore::new(cookie, persist_keys_hint);
        let config = config.unwrap_or_default();
        let timeout_secs = config.timeout.unwrap_or(30);
        let follow_downstream = config.follow_downstream;

        // println!("Authorization: {}", authorization);
        // std::env::set_var("NO_PROXY", "jenkins.example.com,other.example.com"); // Bypass proxy
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true) // Ignore SSL verification
            .no_proxy() // Ignore proxy to avoid potential DNS resolution failure
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .user_agent("Jenkins CLI")
            .build()
            .expect("Failed to create reqwest client");
        // curl -k --noproxy '*' --user "uusername:token" "http://jenkins_url/api/json"
        Self {
            base_url: base_url.to_string(),
            authorization,
            cookie_store,
            cookie_refresh,
            cookie_refresh_attempted: AtomicBool::new(false),
            client,
            follow_downstream,
            job_url: None,
        }
    }

    /// Retrieves the list of projects from the Jenkins server.
    ///
    /// # Returns
    ///
    /// A `Result` containing a vector of `JenkinsJob` or an `anyhow::Error` if the request fails.
    pub async fn get_projects(&self) -> Result<Vec<JenkinsJob>, anyhow::Error> {
        let tree = Self::generate_tree_param(5);
        let url = format_url(&format!("{}/api/json?tree={tree}&pretty=false", self.base_url));
        let response = self.get_with_refresh(&url).await?;
        let json_response: JenkinsResponse = response.json().await?;
        // println!("get_projects: {:?}", json_response);
        fn extract_jobs(jobs: Vec<JenkinsJob>, parent_path: Option<&str>) -> Vec<JenkinsJob> {
            let mut result = Vec::new();
            for mut job in jobs {
                match job._class.as_str() {
                    // Buildable job types
                    job_type if JENKINS_BUILDABLE_TYPES.contains(&job_type) => {
                        if let Some(path) = parent_path {
                            job.name = format!("{}/{}", path, job.name);
                        }
                        result.push(job);
                    }
                    // Folder type - recursively traverse jobs
                    JENKINS_FOLDER_TYPE => {
                        if let Some(sub_jobs) = job.jobs {
                            let folder_path = parent_path.map_or(job.name.clone(), |p| format!("{}/{}", p, job.name));
                            result.extend(extract_jobs(sub_jobs, Some(&folder_path)));
                        }
                    }
                    // Skip auto-build job types
                    job_type if JENKINS_AUTO_BUILD_TYPES.contains(&job_type) => {}
                    // Skip other unknown types
                    _ => {}
                }
            }
            result
        }

        Ok(extract_jobs(json_response.jobs, None))
    }
    /// Generate the tree parameter for the Jenkins API
    fn generate_tree_param(depth: usize) -> String {
        fn build_tree(depth: usize, fields: &str) -> String {
            if depth == 0 {
                format!("[{fields}]")
            } else {
                format!("[{fields},jobs{}]", build_tree(depth - 1, fields))
            }
        }
        let fields = "name,displayName,url,_class";
        format!("jobs{}", build_tree(depth, fields))
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
        // println!("get_job: {}", url);
        let response = self.get_with_refresh_raw(&url).await?;
        let status = response.status();
        if status.is_success() {
            let xml_response = response.text().await?;
            let parameters = jenkins::parse_job_parameters_from_xml(&xml_response);
            return Ok(parameters);
        }

        if status == StatusCode::FORBIDDEN {
            return self.fetch_job_parameters_from_api(job_url).await;
        }

        Err(self
            .handle_response(Ok(response))
            .await
            .err()
            .unwrap_or_else(|| anyhow!("Request failed")))
    }

    /// Fallback helper that reads parameter metadata via the Jenkins JSON API
    /// when `/config.xml` is not accessible.
    async fn fetch_job_parameters_from_api(&self, job_url: &str) -> Result<Vec<JenkinsJobParameter>, anyhow::Error> {
        let tree = "property[_class,parameterDefinitions[name,description,defaultParameterValue[value],choices,trim,credentialType,required,projectName,filter,_class,type]]";
        let api_url = format_url(&format!("{job_url}/api/json?tree={tree}"));
        let response = self.get_with_refresh(&api_url).await?;
        let json_response: serde_json::Value = response.json().await?;
        // println!("json_response: {:?}", json_response);
        let parameters = jenkins::parse_job_parameters_from_json(&json_response);
        Ok(parameters)
    }

    /// Build branch picker options for GIT_BRANCH-like parameters.
    ///
    /// Order priority is:
    /// 1) manual input
    /// 2) default branch
    /// 3) current local branch
    /// 4) remaining remote branches
    ///
    /// Remove duplicate branch names while preserving first-seen order in the prioritized list.
    #[doc(hidden)]
    pub fn build_branch_options(input: BranchOptionsInput<'_>) -> Vec<String> {
        let mut options = Vec::new();
        options.push(input.manual_input.to_string());
        if let Some(default_branch) = input.default_branch.filter(|value| !value.is_empty()) {
            options.push(default_branch.to_string());
        }
        if let Some(current_branch) = input.current_branch.filter(|value| !value.is_empty()) {
            options.push(current_branch.to_string());
        }
        options.extend(input.branches.iter().cloned());

        let mut seen = HashSet::new();
        options
            .into_iter()
            .filter(|branch| seen.insert(branch.clone()))
            .collect()
    }

    #[doc(hidden)]
    pub fn default_choice_selection(choices: &[String], default_value: &str) -> usize {
        choices.iter().position(|choice| choice == default_value).unwrap_or(0)
    }

    /// Prompts the user to enter values for the given parameter definitions.
    ///
    /// # Arguments
    ///
    /// * `parameter_definitions` - The parameter definitions.
    ///
    /// # Returns
    ///
    /// `Some(HashMap)` with parameters, or `None` if user pressed Ctrl+C to go back
    pub async fn prompt_job_parameters(
        parameter_definitions: Vec<JenkinsJobParameter>,
    ) -> Option<HashMap<String, ParamInfo>> {
        use dialoguer::theme::ColorfulTheme; // ColorfulTheme/SimpleTheme
        let mut parameters = HashMap::new();
        let branches = get_git_branches();
        let branch_names = ["GIT_BRANCH", "gitBranch"];

        for param in parameter_definitions {
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
            let (final_value, param_type) = if let Some(choices) = choices {
                let default_selection = Self::default_choice_selection(&choices, &default_value);
                // Use Select to display the Choice list
                let selection =
                    prompt::handle_selection_opt(prompt::with_prompt_kind(prompt::PromptKind::FuzzySelect, || {
                        dialoguer::FuzzySelect::with_theme(&ColorfulTheme::default())
                            .with_prompt(format!("{}{}", t!("prompt-select", "name" => &fmt_name), fmt_desc))
                            .items(&choices)
                            .default(default_selection)
                            .interact_opt()
                    }));

                match selection {
                    Some(idx) => (choices[idx].clone(), ParamType::Choice),
                    None => return None, // Ctrl+C pressed - go back
                }
            } else if param_type == Some(ParamType::Boolean) {
                let default_bool = default_value.parse::<bool>().unwrap_or(false);
                let value = prompt::handle_confirm_opt(prompt::with_prompt_kind(prompt::PromptKind::Confirm, || {
                    dialoguer::Confirm::with_theme(&ColorfulTheme::default())
                        .with_prompt(format!("{}{}", t!("prompt-confirm", "name" => fmt_name), fmt_desc))
                        .default(default_bool)
                        .show_default(true)
                        .wait_for_newline(false)
                        .interact_opt()
                }));

                match value {
                    Some(v) => (v.to_string(), ParamType::Boolean),
                    None => return None, // Ctrl+C pressed - go back
                }
            } else if param_type == Some(ParamType::Password) {
                let prompt_text = format!("{}{}", t!("prompt-password", "name" => fmt_name), fmt_desc);
                match prompt::password_input(&prompt_text, &default_value) {
                    Some(pwd) if pwd.is_empty() => (default_value.to_string(), ParamType::Password),
                    Some(pwd) => (pwd, ParamType::Password),
                    None => return None, // Ctrl+C pressed - go back
                }
            } else if param_type == Some(ParamType::Text) {
                let prompt_text = format!("{}{}", t!("prompt-text", "name" => fmt_name), fmt_desc);
                match prompt::text_input(&prompt_text, &default_value) {
                    Some(v) => (v, ParamType::Text),
                    None => return None, // Ctrl+C pressed - go back
                }
            } else if !branches.is_empty()
                && branch_names
                    .iter()
                    .any(|&b| name.to_lowercase().contains(&b.to_lowercase()))
            {
                // If the parameter name contains GIT_BRANCH
                let current_branch = get_current_branch();
                let manual_input = t!("manual-input");
                let branch_options = Self::build_branch_options(BranchOptionsInput {
                    branches: &branches,
                    default_branch: Some(&default_value),
                    current_branch: Some(&current_branch),
                    manual_input: &manual_input,
                });

                // Priority: default_value, then current_branch, finally use 0
                let default_selection = branch_options
                    .iter()
                    .position(|b| b == &default_value)
                    .or_else(|| branch_options.iter().position(|b| b == &current_branch))
                    .unwrap_or(0);
                let custom_theme = ColorfulTheme {
                    // active_item_style: console::Style::new(), // Cancel default style
                    ..ColorfulTheme::default()
                };
                let selected_idx =
                    prompt::handle_selection_opt(prompt::with_prompt_kind(prompt::PromptKind::FuzzySelectVim, || {
                        dialoguer::FuzzySelect::with_theme(&custom_theme)
                            .with_prompt(format!(
                                "{}{}",
                                t!("prompt-select-branch", "name" => &fmt_name),
                                fmt_desc
                            ))
                            .items(&branch_options)
                            .default(default_selection)
                            .vim_mode(true) // Esc, j|k
                            .with_initial_text("")
                            .interact_opt()
                    }));

                match selected_idx {
                    Some(idx) if branch_options[idx] == manual_input => {
                        let prompt_text = format!("{}{}", t!("prompt-input", "name" => fmt_name), fmt_desc);
                        match prompt::string_input(&prompt_text, "", trim) {
                            Some(v) => (v, ParamType::String),
                            None => return None, // Ctrl+C in manual input
                        }
                    }
                    Some(idx) => (branch_options[idx].clone(), ParamType::String),
                    None => return None, // Ctrl+C pressed - go back
                }
            } else {
                // For other types, use text input
                let prompt_text = format!("{}{}", t!("prompt-input", "name" => fmt_name), fmt_desc);
                match prompt::string_input(&prompt_text, &default_value, trim) {
                    Some(v) => (v, param_type.unwrap_or(ParamType::String)),
                    None => return None, // Ctrl+C pressed
                }
            };

            parameters.insert(
                name,
                ParamInfo {
                    value: final_value,
                    r#type: param_type,
                },
            );
        }
        Some(parameters)
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
        parameters: HashMap<String, ParamInfo>,
    ) -> Result<String, anyhow::Error> {
        // Triggering with format!("{}/build?delay=0sec", job_url) doesn't use a queue
        let params: HashMap<String, String> = parameters
            .into_iter()
            .filter(|(_, v)| v.value != DEFAULT_PARAM_VALUE)
            .map(|(k, v)| (k, v.value))
            .collect();

        let url = format_url(&format!(
            "{}/{}",
            job_url,
            if params.is_empty() {
                "build"
            } else {
                "buildWithParameters"
            }
        ));

        let response = self.post_with_crumb_retry(&url, Some(&params)).await?;
        // queue URL, e.g. http://jenkins_url/queue/item/1/
        let queue_location = response
            .headers()
            .get("Location")
            .ok_or_else(|| anyhow!("Missing Location header"))?
            .to_str()?;
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
        let mut spinner = Some(spinner::Spinner::new(t!("polling-queue-item")));
        let mut paused = false;

        loop {
            tokio::select! {
                _ = delay(2 * 1000) => {
                    if paused {
                        continue;
                    }
                    let response = self.get_with_refresh(&api_url).await?;
                    let queue_item: serde_json::Value = response.json().await?;
                    // println!("{}, queue: {:?}", api_url, queue_item);
                    if let Some(executable) = queue_item["executable"].as_object() {
                        // if let Some(build_url) = executable["url"].as_str() // maybe domain is different
                        if let Some(number) = executable["number"].as_i64() {
                            let job_url = self.job_url.as_ref().unwrap();
                            let build_url = format_url(&format!("{}/{}", job_url, number));
                            if let Some(sp) = spinner.take() {
                                sp.finish_with_message(format!("Build URL: {}", build_url.underline().blue()));
                            } else {
                                println!("Build URL: {}", build_url.underline().blue());
                            }
                            break Ok(build_url.to_string());
                        }
                    }
                },
                msg = event_receiver.recv() => {
                    match msg {
                        Some(Event::StopSpinner) => {
                            if let Some(sp) = spinner.take() {
                                sp.finish_with_message("".to_string());
                            }
                            paused = true;
                        }
                        Some(Event::ResumeSpinner) => {
                            if spinner.is_none() {
                                spinner = Some(spinner::Spinner::new(t!("polling-queue-item")));
                            }
                            paused = false;
                        }
                        Some(Event::CancelPolling) | None => {
                            if let Some(sp) = spinner.take() {
                                sp.finish_with_message("".to_string());
                            }
                            break Err(anyhow!("cancelled!"));
                        }
                    }
                },
            }
        }
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
        let mut visited_builds = HashSet::new();
        self.poll_build_status_inner(build_url, event_receiver, true, &mut visited_builds)
            .await
    }

    async fn poll_build_status_inner(
        &self,
        build_url: &str,
        event_receiver: &mut mpsc::Receiver<Event>,
        allow_downstream: bool,
        visited_builds: &mut HashSet<String>,
    ) -> Result<(), anyhow::Error> {
        let normalized_build_url = normalize_upstream_url(build_url);
        if !visited_builds.insert(normalized_build_url) {
            return Ok(());
        }

        let api_url = format_url(&format!("{}/api/json", build_url));
        let mut spinner = Some(spinner::Spinner::new("".to_string()));
        let mut paused = false;
        let mut last_log_offset = 0; // Initialize the offset of the last read log
        let mut recent_console_html = String::new();
        let mut downstream_jobs = Vec::new();
        let mut downstream_hrefs = HashSet::new();
        let should_follow_downstream = allow_downstream && self.follow_downstream;
        let upstream_info = if should_follow_downstream {
            self.get_build_identity(build_url).await.ok()
        } else {
            None
        };
        let mut located_downstream_builds = Vec::new();
        let mut located_downstream_hrefs = HashSet::new();
        let mut next_downstream_lookup = tokio::time::Instant::now();
        loop {
            tokio::select! {
                _ = delay((1000.0 * 0.2) as u64) => {
                    if paused {
                        continue;
                    }
                    let response = self.get_with_refresh(&api_url).await?;
                    let build_info: serde_json::Value = response.json().await?;

                    // Retrieve and print the incremental part of Jenkins console log
                    match self.get_jenkins_progressive_html(build_url, last_log_offset).await {
                        Ok((html, new_offset)) => {
                            let log = console_html::render_console_html(&html);
                            if should_follow_downstream {
                                recent_console_html.push_str(&html);
                                trim_recent_console_html(&mut recent_console_html);
                                for link in console_html::extract_downstream_links(&recent_console_html) {
                                    if downstream_hrefs.insert(link.key()) {
                                        downstream_jobs.push(link);
                                    }
                                }
                            }
                            if let Some(sp) = spinner.as_ref() {
                                sp.suspend(|| {
                                    terminal::print_stream(&log);
                                });
                            } else {
                                terminal::print_stream(&log);
                            }
                            last_log_offset = new_offset;

                            if should_follow_downstream && tokio::time::Instant::now() >= next_downstream_lookup {
                                if let Some(upstream) = upstream_info.as_ref() {
                                    for job in &downstream_jobs {
                                        if located_downstream_hrefs.contains(&job.key()) {
                                            continue;
                                        }
                                        if let Ok(Some(build)) = self.locate_downstream_build(job, upstream).await {
                                            located_downstream_hrefs.insert(job.key());
                                            located_downstream_builds.push(build);
                                        }
                                    }
                                }
                                next_downstream_lookup = tokio::time::Instant::now() + Duration::from_secs(2);
                            }
                        }
                        Err(e) => {
                            if let Some(sp) = spinner.as_ref() {
                                sp.suspend(|| {
                                  println!("Failed to retrieve console log: {}", e);
                                });
                            } else {
                                println!("Failed to retrieve console log: {}", e);
                            }
                        }
                    }

                    if build_info["building"].as_bool().unwrap_or(false) {
                        delay((1000.0 * 0.5) as u64).await;
                    } else {
                        let result = build_info["result"].as_str().unwrap_or("UNKNOWN"); // or inProgress
                        finish_terminal_line();
                        let build_result = if result == "SUCCESS" {
                            if let Some(sp) = spinner.take() {
                                sp.finish_with_message(format!("Build result: {}", result.bold().green()));
                            } else {
                                println!("Build result: {}", result.bold().green());
                            }
                            Ok(())
                        } else {
                            if let Some(sp) = spinner.take() {
                                sp.finish_with_message(format!("Build result: {}", result.bold().red()));
                            } else {
                                println!("Build result: {}", result.bold().red());
                            }
                            Err(anyhow!(result.red()))
                        };
                        if should_follow_downstream {
                            self.maybe_follow_downstream_builds(
                                build_url,
                                &downstream_jobs,
                                located_downstream_builds,
                                upstream_info,
                                event_receiver,
                                visited_builds,
                            ).await?;
                        }
                        return build_result;
                    }
                },
                msg = event_receiver.recv() => {
                    match msg {
                        Some(Event::StopSpinner) => {
                            reset_terminal_line();
                            if let Some(sp) = spinner.take() {
                                sp.finish_with_message("".to_string());
                            }
                            paused = true;
                        }
                        Some(Event::ResumeSpinner) => {
                            if spinner.is_none() {
                                spinner = Some(spinner::Spinner::new("".to_string()));
                            }
                            paused = false;
                        }
                        Some(Event::CancelPolling) | None => {
                            reset_terminal_line();
                            if let Some(sp) = spinner.take() {
                                sp.finish_with_message("".to_string());
                            }
                            return Err(anyhow!("cancelled!"));
                        }
                    }
                },
                // _ = spawn_and_handle_enter_key() => {
                // },
            }
        }
    }

    /// Retrieves the incremental part of the Jenkins build log
    pub async fn get_jenkins_progressive_html(
        &self,
        build_url: &str,
        start: usize,
    ) -> Result<(String, usize), anyhow::Error> {
        let api_url = format_url(&format!("{}/logText/progressiveHtml?start={}", build_url, start));
        let response = self.get_with_refresh(&api_url).await?;

        // Get the new length from the 'X-Text-Size' header
        let new_offset = response
            .headers()
            .get("X-Text-Size")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(start);

        let console_log = response.text().await?;

        Ok((console_log, new_offset))
    }

    async fn maybe_follow_downstream_builds(
        &self,
        upstream_build_url: &str,
        downstream_jobs: &[DownstreamJobLink],
        prelocated_builds: Vec<DownstreamBuild>,
        upstream_info: Option<BuildIdentity>,
        event_receiver: &mut mpsc::Receiver<Event>,
        visited_builds: &mut HashSet<String>,
    ) -> Result<(), anyhow::Error> {
        if downstream_jobs.is_empty() {
            return Ok(());
        }

        reset_terminal_line();
        terminal::print_line(t!("downstream-jobs-detected").bold());
        for job in downstream_jobs {
            terminal::print_line(format!("  - {}", job.label.cyan()));
        }

        finish_terminal_line();
        let upstream_info = match upstream_info {
            Some(upstream) => upstream,
            None => self.get_build_identity(upstream_build_url).await?,
        };
        let mut located = prelocated_builds;
        let mut located_hrefs = located.iter().map(|build| build.job.key()).collect::<HashSet<String>>();
        for job in downstream_jobs {
            if located_hrefs.contains(&job.key()) {
                continue;
            }
            match self.locate_downstream_build_with_retry(job, &upstream_info).await {
                Ok(Some(build)) => {
                    located_hrefs.insert(job.key());
                    located.push(build);
                }
                Ok(None) => {
                    terminal::print_line(t!("downstream-build-not-found", "name" => job.label.clone()).yellow())
                }
                Err(e) => terminal::print_line(
                    t!("downstream-build-lookup-failed", "name" => job.label.clone(), "error" => e.to_string())
                        .yellow(),
                ),
            }
        }

        let mut first_error = None;
        for build in located {
            finish_terminal_line();
            terminal::print_line(terminal::separator(&t!("downstream-build-title")).dimmed());
            terminal::print_line(format!(
                "{} {}",
                t!("following-downstream-build", "name" => build.job.label.clone()),
                build.build_url.underline().blue()
            ));
            if let Err(e) =
                Box::pin(self.poll_build_status_inner(&build.build_url, event_receiver, true, visited_builds)).await
            {
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }

        if let Some(e) = first_error {
            return Err(e);
        }

        Ok(())
    }

    async fn get_build_identity(&self, build_url: &str) -> Result<BuildIdentity, anyhow::Error> {
        let api_url = format_url(&format!("{}/api/json?tree=number,url", build_url));
        let response = self.get_with_refresh(&api_url).await?;
        let build_info: serde_json::Value = response.json().await?;
        let number = build_info["number"]
            .as_u64()
            .ok_or_else(|| anyhow!("missing upstream build number"))?;
        let url = build_info["url"].as_str().unwrap_or(build_url).to_string();
        let build_relative_url = self
            .relative_jenkins_url(&url)
            .unwrap_or_else(|| relative_path_from_url(&url));
        let upstream_url = build_relative_url_to_job_url(&build_relative_url, number);

        Ok(BuildIdentity {
            number,
            upstream_url: normalize_upstream_url(&upstream_url),
        })
    }

    async fn locate_downstream_build(
        &self,
        job: &DownstreamJobLink,
        upstream: &BuildIdentity,
    ) -> Result<Option<DownstreamBuild>, anyhow::Error> {
        let Some(job_url) = self.resolve_downstream_job_url(job).await? else {
            return Ok(None);
        };
        let api_url = format_url(&format!(
            "{}/api/json?tree=builds[number,url,building,result,actions[causes[*]]]{{0,20}}",
            job_url
        ));
        let response = self.get_with_refresh(&api_url).await?;
        let job_info: serde_json::Value = response.json().await?;
        let Some(builds) = job_info["builds"].as_array() else {
            return Ok(None);
        };

        for build in builds {
            if build_matches_upstream(build, upstream) {
                if let Some(url) = build["url"].as_str() {
                    return Ok(Some(DownstreamBuild {
                        job: job.clone(),
                        build_url: url.to_string(),
                    }));
                }
            }
        }

        Ok(None)
    }

    async fn resolve_downstream_job_url(&self, job: &DownstreamJobLink) -> Result<Option<String>, anyhow::Error> {
        Ok(Some(self.absolute_jenkins_url(&job.href)))
    }

    async fn locate_downstream_build_with_retry(
        &self,
        job: &DownstreamJobLink,
        upstream: &BuildIdentity,
    ) -> Result<Option<DownstreamBuild>, anyhow::Error> {
        let attempts = 15;
        for attempt in 0..attempts {
            if let Some(build) = self.locate_downstream_build(job, upstream).await? {
                return Ok(Some(build));
            }

            if attempt + 1 < attempts {
                delay(2000).await;
            }
        }

        Ok(None)
    }

    fn absolute_jenkins_url(&self, href: &str) -> String {
        if href.starts_with("http://") || href.starts_with("https://") {
            format_url(href)
        } else {
            format_url(&format!(
                "{}/{}",
                self.base_url.trim_end_matches('/'),
                href.trim_start_matches('/')
            ))
        }
    }

    fn relative_jenkins_url(&self, url: &str) -> Option<String> {
        let base = reqwest::Url::parse(&self.base_url).ok()?;
        let parsed = reqwest::Url::parse(url).ok()?;
        if base.scheme() != parsed.scheme() || base.host_str() != parsed.host_str() || base.port() != parsed.port() {
            return None;
        }

        Some(parsed.path().trim_start_matches('/').to_string())
    }

    /// Get Jenkins build log
    #[allow(dead_code)]
    pub async fn get_jenkins_console_log(&self, build_url: &str) -> Result<(), anyhow::Error> {
        let api_url = format_url(&format!("{}/consoleText", build_url));
        let response = self.get_with_refresh(&api_url).await?;
        let console_log = response.text().await?;
        clear_screen();
        println!("{}", console_log);
        Ok(())
    }

    /// Check if there is an ongoing build and return the build status and number
    pub async fn is_building(&self) -> Result<BuildStatus, anyhow::Error> {
        let job_url = self.job_url.as_ref().unwrap();
        let job_api_url = format_url(&format!(
            "{}/api/json?tree=inQueue,lastBuild[number],lastCompletedBuild[number]",
            job_url
        ));
        let response = self.get_with_refresh(&job_api_url).await?;
        let job_info: serde_json::Value = response.json().await?;

        let last_build_num = job_info["lastBuild"]["number"].as_u64().map(|n| n as u32);
        let last_completed_num = job_info["lastCompletedBuild"]["number"].as_u64().map(|n| n as u32);
        let in_queue = job_info["inQueue"].as_bool().unwrap_or(false);

        if let (Some(last), Some(completed)) = (last_build_num, last_completed_num) {
            if last > completed {
                return Ok(BuildStatus {
                    building: true,
                    id: Some(last),
                    last_build: last_build_num,
                    last_completed: last_completed_num,
                    in_queue,
                });
            }
        }

        let api_url = format_url(&format!("{}/lastBuild/api/json", job_url));
        let response = self.get_with_refresh(&api_url).await?;
        let build_info: serde_json::Value = response.json().await?;
        let is_building = build_info["building"].as_bool().unwrap_or(false);
        let build_number = build_info["number"].as_u64().map(|n| n as u32);
        if !is_building && !in_queue {
            let builds_api_url = format_url(&format!("{}/api/json?tree=builds[number,building]", job_url));
            if let Ok(response) = self.get_with_refresh(&builds_api_url).await {
                if let Ok(builds_info) = response.json::<serde_json::Value>().await {
                    if let Some(builds) = builds_info["builds"].as_array() {
                        if let Some(running) = builds.iter().find(|b| b["building"].as_bool().unwrap_or(false)) {
                            let running_id = running["number"].as_u64().map(|n| n as u32);
                            return Ok(BuildStatus {
                                building: true,
                                id: running_id,
                                last_build: last_build_num,
                                last_completed: last_completed_num,
                                in_queue,
                            });
                        }
                    }
                }
            }
        }
        Ok(BuildStatus {
            building: is_building,
            id: build_number,
            last_build: last_build_num,
            last_completed: last_completed_num,
            in_queue,
        })
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
        match self.post_with_crumb_retry(&api_url, None).await {
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
    /// Get project info
    pub async fn get_project(&self, job_url: &str) -> Result<JenkinsJob, Box<dyn std::error::Error>> {
        let api_url = format_url(&format!("{}/api/json", job_url));
        let response = self.get_with_refresh(&api_url).await?;
        let project: JenkinsJob = response.json().await?;
        Ok(project)
    }
}

fn normalize_upstream_url(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

fn relative_path_from_url(url: &str) -> String {
    reqwest::Url::parse(url)
        .map(|parsed| parsed.path().trim_start_matches('/').to_string())
        .unwrap_or_else(|_| url.trim_start_matches('/').to_string())
}

fn build_relative_url_to_job_url(build_relative_url: &str, build_number: u64) -> String {
    let suffix = format!("/{build_number}");
    let normalized = normalize_upstream_url(build_relative_url);
    normalized
        .strip_suffix(&suffix)
        .unwrap_or(normalized.as_str())
        .to_string()
}

fn build_matches_upstream(build: &serde_json::Value, upstream: &BuildIdentity) -> bool {
    let Some(actions) = build["actions"].as_array() else {
        return false;
    };

    for action in actions {
        let Some(causes) = action["causes"].as_array() else {
            continue;
        };

        for cause in causes {
            let upstream_build = cause["upstreamBuild"].as_u64();
            let upstream_url = cause["upstreamUrl"].as_str().map(normalize_upstream_url);

            if upstream_build == Some(upstream.number)
                && upstream_url.as_deref() == Some(upstream.upstream_url.as_str())
            {
                return true;
            }
        }
    }

    false
}

fn trim_recent_console_html(html: &mut String) {
    const MAX_RECENT_HTML_BYTES: usize = 64 * 1024;
    if html.len() <= MAX_RECENT_HTML_BYTES {
        return;
    }

    let keep_from = html
        .char_indices()
        .map(|(idx, _)| idx)
        .find(|idx| html.len() - idx <= MAX_RECENT_HTML_BYTES)
        .unwrap_or(0);
    html.drain(..keep_from);
}

#[cfg(test)]
mod tests {
    use super::{build_matches_upstream, BuildIdentity};
    use serde_json::json;

    #[test]
    fn build_matches_upstream_by_number_and_url() {
        let upstream = BuildIdentity {
            number: 123,
            upstream_url: "job/forder-test/job/main".to_string(),
        };
        let build = json!({
            "actions": [
                {
                    "causes": [
                        {
                            "upstreamBuild": 123,
                            "upstreamUrl": "job/forder-test/job/main/"
                        }
                    ]
                }
            ]
        });

        assert!(build_matches_upstream(&build, &upstream));
    }

    #[test]
    fn build_does_not_match_different_upstream() {
        let upstream = BuildIdentity {
            number: 123,
            upstream_url: "job/forder-test/job/main".to_string(),
        };
        let build = json!({
            "actions": [
                {
                    "causes": [
                        {
                            "upstreamBuild": 456,
                            "upstreamUrl": "job/forder-test/job/main/"
                        }
                    ]
                }
            ]
        });

        assert!(!build_matches_upstream(&build, &upstream));
    }

    #[test]
    fn converts_build_relative_url_to_job_url() {
        assert_eq!(
            super::build_relative_url_to_job_url("job/forder-test/job/main/123/", 123),
            "job/forder-test/job/main"
        );
    }
}
