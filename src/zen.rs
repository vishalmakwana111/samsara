//! Minimal opencode Zen API client — used to validate keys.
//!
//! Zen models endpoint: `GET https://opencode.ai/zen/v1/models`. Accepts the key as
//! `Authorization: Bearer <key>` (OpenAI-style) — a 200 means the key is live, a 401/403
//! means it's rejected. We use this to sanity-check a key when it's added.

const MODELS_URL: &str = "https://opencode.ai/zen/v1/models";

/// The result of validating a key against Zen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Validity {
    /// Key accepted; `models` is how many models the response listed (0 if unknown).
    Ok { models: usize },
    /// Key rejected (401/403).
    Unauthorized,
    /// Some other HTTP status.
    Other(u16),
    /// Could not reach Zen at all.
    Unreachable(String),
}

/// Validate a Zen API key by listing models.
pub async fn validate(key: &str) -> Validity {
    let client = match reqwest::Client::builder()
        .user_agent(concat!("samsara/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => return Validity::Unreachable(e.to_string()),
    };

    let resp = client
        .get(MODELS_URL)
        .bearer_auth(key)
        .header("x-api-key", key)
        .send()
        .await;

    match resp {
        Ok(r) => {
            let status = r.status();
            if status.is_success() {
                let models = r
                    .json::<serde_json::Value>()
                    .await
                    .ok()
                    .and_then(|v| count_models(&v))
                    .unwrap_or(0);
                Validity::Ok { models }
            } else if status.as_u16() == 401 || status.as_u16() == 403 {
                Validity::Unauthorized
            } else {
                Validity::Other(status.as_u16())
            }
        }
        Err(e) => Validity::Unreachable(e.to_string()),
    }
}

/// Count models from either `{ "data": [...] }` (OpenAI) or a bare array.
fn count_models(v: &serde_json::Value) -> Option<usize> {
    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
        return Some(arr.len());
    }
    v.as_array().map(|a| a.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_openai_and_bare_shapes() {
        let a = serde_json::json!({"data":[{"id":"x"},{"id":"y"}]});
        assert_eq!(count_models(&a), Some(2));
        let b = serde_json::json!([{"id":"x"}]);
        assert_eq!(count_models(&b), Some(1));
        let c = serde_json::json!({"nope": true});
        assert_eq!(count_models(&c), None);
    }
}
