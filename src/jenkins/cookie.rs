use std::collections::{HashMap, HashSet};

use reqwest::header::SET_COOKIE;

/// Cookie handling with in-memory updates + optional persistence of configured keys.
pub struct CookieStore {
    // Current in-memory cookie header (may include transient keys like JSESSIONID).
    current: std::sync::Mutex<Option<String>>,
    // Only persist the keys that were explicitly configured (e.g. jwt_token).
    persist_keys: Option<HashSet<String>>,
    // Last persisted cookie for configured keys (normalized key-value string), used to avoid repeated writes.
    persisted: std::sync::Mutex<Option<String>>,
}

impl CookieStore {
    pub fn new(initial_cookie: Option<&str>, persist_keys_hint: Option<HashSet<String>>) -> Self {
        let cookie_value = initial_cookie.map(|value| value.to_string());
        let (persist_keys, persisted) = if let Some(keys) = persist_keys_hint.filter(|set| !set.is_empty()) {
            let normalized = cookie_value
                .as_deref()
                .map(|value| Self::filter_cookie_string(value, &keys))
                .filter(|value| !value.is_empty());
            (Some(keys), normalized)
        } else {
            match cookie_value.as_deref() {
                Some(raw) => {
                    let map = Self::parse_cookie_map(raw);
                    if map.is_empty() {
                        (None, None)
                    } else {
                        let keys = map.keys().cloned().collect::<HashSet<String>>();
                        let normalized = Self::cookie_map_to_string(map);
                        (Some(keys), Some(normalized))
                    }
                }
                None => (None, None),
            }
        };

        Self {
            current: std::sync::Mutex::new(cookie_value),
            persist_keys,
            persisted: std::sync::Mutex::new(persisted),
        }
    }

    pub fn header_value(&self) -> Option<String> {
        self.current.lock().unwrap().clone()
    }

    pub fn get_value(&self, name: &str) -> Option<String> {
        let current = self.current.lock().unwrap().clone().unwrap_or_default();
        let map = Self::parse_cookie_map(&current);
        map.get(name).cloned()
    }

    pub fn update_from_response(&self, response: &reqwest::Response, base_url: &str) {
        let mut updates = Vec::new();
        for value in response.headers().get_all(SET_COOKIE).iter() {
            if let Ok(raw) = value.to_str() {
                if let Some((name, val)) = Self::parse_cookie_pair(raw) {
                    updates.push((name, val));
                }
            }
        }
        // Apply Set-Cookie updates and persist configured keys if needed.
        self.apply_updates(updates, base_url);
    }

    // Apply cookie updates from explicit name/value pairs.
    pub fn update_from_pairs(&self, updates: Vec<(String, String)>, base_url: &str) {
        self.apply_updates(updates, base_url);
    }

    fn parse_cookie_pair(raw: &str) -> Option<(String, String)> {
        let pair = raw.split(';').next().unwrap_or("").trim();
        let mut parts = pair.splitn(2, '=');
        let name = parts.next()?.trim();
        let value = parts.next()?.trim();
        if name.is_empty() {
            return None;
        }
        Some((name.to_string(), value.to_string()))
    }

    /// Parse "a=b; c=d" into a map. Ignores invalid entries.
    fn parse_cookie_map(cookie: &str) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for part in cookie.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let mut parts = part.splitn(2, '=');
            if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
                let name = name.trim();
                let value = value.trim();
                if !name.is_empty() {
                    map.insert(name.to_string(), value.to_string());
                }
            }
        }
        map
    }

    // Keep only configured cookie keys, for persistence.
    fn filter_cookie_string(cookie: &str, keys: &HashSet<String>) -> String {
        let map = Self::parse_cookie_map(cookie);
        let mut keep = HashMap::new();
        for (k, v) in map {
            if keys.contains(&k) {
                keep.insert(k, v);
            }
        }
        Self::cookie_map_to_string(keep)
    }

    fn merge_cookies(existing: &str, updates: Vec<(String, String)>) -> String {
        let mut map: HashMap<String, String> = Self::parse_cookie_map(existing);
        for (name, value) in updates {
            map.insert(name, value);
        }
        Self::cookie_map_to_string(map)
    }

    // Merge updates into current cookie, then persist configured keys.
    fn apply_updates(&self, updates: Vec<(String, String)>, base_url: &str) {
        if updates.is_empty() {
            return;
        }
        if crate::utils::debug_enabled() {
            let keys: Vec<_> = updates.iter().map(|(k, _)| k.as_str()).collect();
            eprintln!("[debug] cookie: applying updates for keys {:?}", keys);
        }

        let merged = {
            let mut current_guard = self.current.lock().unwrap();
            let existing = current_guard.clone().unwrap_or_default();
            let merged = Self::merge_cookies(&existing, updates);
            if !merged.is_empty() {
                *current_guard = Some(merged.clone());
            }
            merged
        };

        if merged.is_empty() {
            return;
        }
        // Avoid noisy debug logs for full cookie values.

        // Persist only configured keys (e.g. jwt_token), avoid transient keys like JSESSIONID.
        let persist_keys = match self.persist_keys.as_ref() {
            Some(keys) if !keys.is_empty() => keys,
            _ => return,
        };
        let subset = Self::filter_cookie_string(&merged, persist_keys);
        if subset.is_empty() {
            return;
        }
        let mut persisted_guard = self.persisted.lock().unwrap();
        let previous = persisted_guard.clone();
        if persisted_guard.as_deref() == Some(subset.as_str()) {
            return;
        }
        // Only write to config when the persisted subset actually changes.
        let persisted_result = crate::config::persist_cookie_for_url(base_url, &subset).unwrap_or(false);
        if persisted_result {
            *persisted_guard = Some(subset);
        }
        if crate::utils::debug_enabled() {
            if persisted_result {
                eprintln!("[debug] cookie: persisted (previous={:?})", previous);
            } else {
                eprintln!("[debug] cookie: persist skipped");
            }
        }
    }

    // Stable serialization (sort keys) for comparisons and config writes.
    fn cookie_map_to_string(map: HashMap<String, String>) -> String {
        let mut items: Vec<(String, String)> = map.into_iter().collect();
        items.sort_by(|a, b| a.0.cmp(&b.0));
        items
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<String>>()
            .join("; ")
    }
}
