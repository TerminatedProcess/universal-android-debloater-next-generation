//! AI enrichment for unknown ("Unlisted") packages.
//!
//! UAD's curated `uad_lists.json` covers most system packages, but third-party
//! apps and unknown system packages have no human-readable name, no description,
//! and no safety guidance. This module fills that gap by asking a local
//! multi-provider AI proxy ([`mcp-ai-proxy`], default `http://localhost:6500`)
//! for a friendly name, a one-sentence description, and a remove-safety verdict.
//!
//! Results are cached on disk (keyed by package id) so each unknown package
//! costs at most one network call, ever. Everything degrades gracefully: if the
//! proxy is unreachable, callers simply get `None` and fall back to the raw id.
//!
//! Configuration (env, both optional):
//! - `UAD_AI_PROXY_URL` — base URL of the proxy (default `http://localhost:6500`)
//! - `UAD_AI_PROXY_KEY` — bearer token, only if the proxy sets `PROXY_API_KEYS`

use crate::CACHE_DIR;
use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub const AI_CACHE_FNAME: &str = "ai_enrich.json";

/// One enriched package, as returned by the AI and stored in the cache.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiInfo {
    /// Short human app name, e.g. `Netflix`.
    #[serde(default)]
    pub friendly_name: String,
    /// One concise sentence: what the app/service does.
    #[serde(default)]
    pub description: String,
    /// One of `safe` / `caution` / `unsafe` — whether *removing* it risks
    /// breaking the Android system (not whether the user wants the app).
    #[serde(default)]
    pub safe_to_remove: String,
}

impl AiInfo {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.friendly_name.is_empty() && self.description.is_empty()
    }
}

/// Map of `package id -> AiInfo`.
pub type AiCache = HashMap<String, AiInfo>;

fn cache_path() -> PathBuf {
    CACHE_DIR.join(AI_CACHE_FNAME)
}

/// Load the on-disk enrichment cache. Missing/corrupt file -> empty map.
#[must_use]
pub fn load_cache() -> AiCache {
    match std::fs::read_to_string(cache_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
            warn!("AI cache parse failed, ignoring: {e}");
            AiCache::new()
        }),
        Err(_) => AiCache::new(),
    }
}

/// Persist the enrichment cache to disk (best-effort).
pub fn save_cache(cache: &AiCache) {
    match serde_json::to_string_pretty(cache) {
        Ok(s) => {
            if let Err(e) = std::fs::write(cache_path(), s) {
                warn!("Could not write AI cache: {e}");
            }
        }
        Err(e) => warn!("Could not serialize AI cache: {e}"),
    }
}

fn proxy_url() -> String {
    std::env::var("UAD_AI_PROXY_URL").unwrap_or_else(|_| "http://localhost:6500".to_string())
}

/// Ask the AI proxy to enrich a single package id.
/// Returns `None` on any failure (proxy down, bad JSON, empty answer).
#[must_use]
pub fn enrich_one(pkg_id: &str) -> Option<AiInfo> {
    let url = format!("{}/structured", proxy_url());
    let prompt = format!(
        "Android package id: `{pkg_id}`. Respond with ONLY a JSON object with keys: \
         friendly_name (short human app name, e.g. \"Netflix\"; if genuinely unknown, \
         derive a readable name from the id), \
         description (one concise sentence describing what the app or service does), \
         safe_to_remove (exactly one of: safe, caution, unsafe). For safe_to_remove judge \
         whether REMOVING the package risks breaking the Android system/OS, NOT whether the \
         user personally wants it: a normal user-facing app (game, streaming, browser) is \
         \"safe\"; a component other apps depend on is \"caution\"; a core framework/service \
         is \"unsafe\"."
    );
    let system = "You are a terse Android package expert. Output only valid minified JSON, no prose, no markdown fences.";

    let mut builder = ureq::post(&url);
    if let Ok(key) = std::env::var("UAD_AI_PROXY_KEY") {
        builder = builder.header("Authorization", format!("Bearer {key}"));
    }

    let payload = serde_json::json!({
        "prompt": prompt,
        "system": system,
        "max_tokens": 300,
        "temperature": 0.2,
    });

    match builder.send_json(&payload) {
        Ok(mut resp) => {
            let val: serde_json::Value = resp.body_mut().read_json().ok()?;
            // The proxy wraps the model's JSON under a `data` key.
            let data = val.get("data")?;
            let info: AiInfo = serde_json::from_value(data.clone()).ok()?;
            if info.is_empty() { None } else { Some(info) }
        }
        Err(e) => {
            warn!("AI enrich failed for {pkg_id}: {e}");
            None
        }
    }
}

/// Free-form chat turn against the AI proxy `/chat` endpoint.
/// `system` sets the assistant's role; `prompt` is the full user turn
/// (caller is responsible for embedding any package context and history).
/// Returns the assistant's reply, or `None` on failure.
#[must_use]
pub fn chat(system: &str, prompt: &str) -> Option<String> {
    let url = format!("{}/chat", proxy_url());
    let mut builder = ureq::post(&url);
    if let Ok(key) = std::env::var("UAD_AI_PROXY_KEY") {
        builder = builder.header("Authorization", format!("Bearer {key}"));
    }
    let payload = serde_json::json!({
        "prompt": prompt,
        "system": system,
        "max_tokens": 600,
        "temperature": 0.4,
    });
    match builder.send_json(&payload) {
        Ok(mut resp) => {
            let val: serde_json::Value = resp.body_mut().read_json().ok()?;
            let content = val.get("content")?.as_str()?.trim().to_string();
            if content.is_empty() { None } else { Some(content) }
        }
        Err(e) => {
            warn!("AI chat failed: {e}");
            None
        }
    }
}

/// Enrich many package ids, up to `concurrency` requests at a time.
/// Only successful lookups appear in the returned map.
#[must_use]
pub fn enrich_batch(ids: &[String], concurrency: usize) -> AiCache {
    let mut out = AiCache::new();
    for chunk in ids.chunks(concurrency.max(1)) {
        let results: Vec<(String, Option<AiInfo>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|id| scope.spawn(move || (id.clone(), enrich_one(id))))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or((String::new(), None)))
                .collect()
        });
        for (id, info) in results {
            if let Some(info) = info {
                out.insert(id, info);
            }
        }
    }
    out
}
