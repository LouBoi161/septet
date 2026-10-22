//! Ollama's models hold graphics memory for minutes after each answer. Before a job that does
//! not fit, Septet asks Ollama to unload them (`keep_alive: 0`); Ollama loads them again on its
//! next request. Nothing is ended.

use std::time::Duration;

use serde_json::{Value, json};

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(2)))
        .timeout_global(Some(Duration::from_secs(20)))
        .max_redirects(0)
        .build()
        .new_agent()
}

/// The models Ollama at `base` has loaded (none if it doesn't answer).
pub fn loaded(base: &str) -> Vec<String> {
    let Ok(mut resp) = agent().get(format!("{}/api/ps", base.trim_end_matches('/'))).call() else { return Vec::new() };
    let Ok(text) = resp.body_mut().with_config().limit(1 << 20).read_to_string() else { return Vec::new() };
    let v: Value = serde_json::from_str(&text).unwrap_or_default();
    v["models"].as_array().into_iter().flatten().filter_map(|m| m["name"].as_str().or_else(|| m["model"].as_str()).map(str::to_owned)).collect()
}

/// Ask Ollama to unload `model` now.
pub fn unload(base: &str, model: &str) {
    let body = json!({ "model": model, "keep_alive": 0 }).to_string();
    let _ = agent().post(format!("{}/api/generate", base.trim_end_matches('/'))).header("Content-Type", "application/json").send(body.as_bytes());
}
