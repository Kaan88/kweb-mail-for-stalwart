//! The models a service offers, for the picker in the settings. Sends no mail. Same care as
//! [`super::provider`]: the address is checked first, nothing is redirected, the key is only
//! ever a sensitive header, and the service's own words are never put into an error.

use std::time::Duration;

use serde_json::Value;

use super::{
    provider::{self, effort_named, host_is, read_capped, sensitive, BodyError, MAX_BODY},
    settings,
};
use crate::{
    model::{AiEffort, AiModel, AiModelList},
    Error, Result,
};

/// To reach the service.
const CONNECT: Duration = Duration::from_secs(10);
/// For the whole request, the body included.
const WHOLE: Duration = Duration::from_secs(30);

/// The models of the service at `base_url` (already trimmed, without a trailing slash), from its
/// `/models`. `key` is sent as a bearer header, when there is one.
pub async fn list(base_url: &str, key: Option<&str>) -> Result<AiModelList> {
    let url = settings::check_base_url(base_url)?;
    let host = settings::host_label(&url);
    let headers: Vec<(&str, String)> = key.iter().map(|k| ("authorization", format!("Bearer {k}"))).collect();
    let answer = get(&format!("{base_url}/models"), &headers, &host).await?;
    Ok(AiModelList { models: openai_models(&answer, base_url)? })
}

fn unreadable() -> Error {
    Error::temporary("the service gave an answer that could not be read")
}

/// `{"data":[{"id":..}]}`, by id, without the models that cannot answer a chat request (see
/// [`takes_chat`]). The name is the service's `name` when it gives one. The effort levels are the
/// ones the service lists for the model (`reasoning.supported_efforts`, as OpenRouter gives them);
/// a service that lists none gets none, and then no effort is offered or sent.
fn openai_models(answer: &Value, base_url: &str) -> Result<Vec<AiModel>> {
    let data = answer["data"].as_array().ok_or_else(unreadable)?;
    let gemini = host_is(base_url, "generativelanguage.googleapis.com");
    let openai = host_is(base_url, "api.openai.com");
    let mut models: Vec<AiModel> = data
        .iter()
        .filter(|m| takes_chat(m, openai))
        .filter_map(|m| {
            let listed = m["id"].as_str()?;
            // Gemini lists `models/<id>` and documents the bare id for chat requests.
            let id = if gemini { listed.strip_prefix("models/").unwrap_or(listed) } else { listed };
            let id = id.trim();
            if id.is_empty() {
                return None;
            }
            let name = m["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(id);
            Some(AiModel { id: id.into(), name: name.into(), efforts: openai_levels(m) })
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    Ok(models)
}

/// Words in a model id that mark a model for something else than chat: embeddings, speech,
/// pictures, video, moderation, live audio. Such a model answers a chat request with an error.
const NOT_CHAT_WORDS: [&str; 14] = [
    "embed",
    "tts",
    "whisper",
    "transcribe",
    "dall-e",
    "gpt-image",
    "imagen",
    "veo-",
    "moderation",
    "realtime",
    "sora",
    "rerank",
    "aqa",
    "-image-generation",
];

/// Words of OpenAI model ids that OpenAI serves through its Responses API only (the chat
/// interface answers 404 for them).
const RESPONSES_ONLY_WORDS: [&str; 4] = ["-pro", "codex", "deep-research", "computer-use"];

/// Whether the entry `m` of a list of models can answer a chat request, as far as the list says:
/// an entry the service marks as for other interfaces only (OpenRouter's `endpoints`, which has
/// batch-only `:batch` twins of its models), not for chat (Mistral's `capabilities.completion_chat`)
/// or inactive (Groq's `active`) is left out, and so is one whose id names another kind of model.
/// `openai`: the list is OpenAI's own, where some models answer only through the Responses API.
fn takes_chat(m: &Value, openai: bool) -> bool {
    if let Some(paths) = m["endpoints"].as_array() {
        if !paths.iter().filter_map(Value::as_str).any(|path| path.ends_with("/chat/completions")) {
            return false;
        }
    }
    if m["capabilities"]["completion_chat"].as_bool() == Some(false) || m["active"].as_bool() == Some(false) {
        return false;
    }
    let id = m["id"].as_str().unwrap_or_default().to_ascii_lowercase();
    if NOT_CHAT_WORDS.iter().any(|word| id.contains(word)) {
        return false;
    }
    !(openai && RESPONSES_ONLY_WORDS.iter().any(|word| id.contains(word)))
}

/// The effort levels of one entry, lowest first, as the service lists them
/// (`reasoning.supported_efforts`); empty when it lists none or only words the app does not know.
fn openai_levels(m: &Value) -> Vec<AiEffort> {
    let mut levels: Vec<AiEffort> = m["reasoning"]["supported_efforts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|word| effort_named(&word.trim().to_ascii_lowercase()))
        .collect();
    levels.sort_by_key(|e| *e as u8);
    levels.dedup();
    levels
}

/// One GET, the answer as JSON.
async fn get(url: &str, headers: &[(&str, String)], host: &str) -> Result<Value> {
    let client = provider::client(CONNECT, Some(WHOLE))?;
    let mut request = client.get(url).header("accept", "application/json");
    for (name, value) in headers {
        request = request.header(*name, sensitive(value)?);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(e) if e.is_timeout() => return Err(Error::temporary("the service did not answer in time")),
        Err(_) => return Err(Error::temporary(format!("could not reach the service at {host}"))),
    };
    if !response.status().is_success() {
        return Err(provider::refused(response, WHOLE).await);
    }
    let body = match read_capped(response, MAX_BODY, WHOLE).await {
        Ok(body) => body,
        Err(BodyError::TooLarge) => return Err(Error::temporary("the list of models is too long")),
        Err(BodyError::Broken) => return Err(Error::temporary("the service stopped answering")),
    };
    serde_json::from_slice(&body).map_err(|_| unreadable())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ids(models: &[AiModel]) -> Vec<&str> {
        models.iter().map(|m| m.id.as_str()).collect()
    }

    #[test]
    fn openai_models_are_sorted_by_id_and_have_no_efforts_unless_listed() {
        let answer = json!({"data": [{"id": "b"}, {"id": "a", "owned_by": "x"}, {"nope": 1}]});
        let models = openai_models(&answer, "https://llm.example/v1").unwrap();
        assert_eq!(ids(&models), ["a", "b"]);
        assert!(models.iter().all(|m| m.name == m.id && m.efforts.is_empty()));
        assert!(matches!(openai_models(&json!({"models": []}), "https://x.example/v1"), Err(Error::Temporary { .. })));
    }

    #[test]
    fn openrouter_entries_bring_their_name_and_levels_and_batch_twins_are_left_out() {
        let answer = json!({"data": [
            {"id": "anthropic/claude-opus-5.5", "name": "Anthropic: Claude Opus 5.5",
             "endpoints": ["/api/v1/chat/completions", "/api/v1/messages"],
             "reasoning": {"mandatory": true, "supported_efforts": ["max", "xhigh", "high", "medium", "low"]}},
            {"id": "anthropic/claude-opus-5.5:batch", "endpoints": ["/api/v1/batches"],
             "reasoning": {"supported_efforts": ["high"]}},
            {"id": "openai/gpt-5.5", "reasoning": {"supported_efforts": ["xhigh", "none", "medium", "minimal", "odd", "medium"]}},
            {"id": "meta/plain", "reasoning": null},
        ]});
        let models = openai_models(&answer, "https://openrouter.ai/api/v1").unwrap();
        assert_eq!(ids(&models), ["anthropic/claude-opus-5.5", "meta/plain", "openai/gpt-5.5"]);
        assert_eq!(models[0].name, "Anthropic: Claude Opus 5.5");
        assert_eq!(
            models[0].efforts,
            [AiEffort::Low, AiEffort::Medium, AiEffort::High, AiEffort::XHigh, AiEffort::Max]
        );
        assert!(models[1].efforts.is_empty());
        assert_eq!(models[2].efforts, [AiEffort::Off, AiEffort::Minimal, AiEffort::Medium, AiEffort::XHigh]);
    }

    #[test]
    fn models_that_cannot_chat_are_left_out() {
        let answer = json!({"data": [
            {"id": "mistral-large-latest", "capabilities": {"completion_chat": true, "reasoning": false}},
            {"id": "mistral-ocr-latest", "capabilities": {"completion_chat": false}},
            {"id": "llama-3.3-70b", "active": true},
            {"id": "old-model", "active": false},
            {"id": "text-embedding-3-small"}, {"id": "nomic-embed-text"}, {"id": "whisper-large-v3"},
            {"id": "gpt-4o-mini-tts"}, {"id": "gpt-4o-transcribe"}, {"id": "dall-e-3"}, {"id": "gpt-image-1"},
            {"id": "omni-moderation-latest"}, {"id": "gpt-realtime"}, {"id": "sora-2"},
            {"id": "gpt-5.5-pro"}, {"id": "gpt-5-codex"},
        ]});
        let other = openai_models(&answer, "https://llm.example/v1").unwrap();
        assert_eq!(ids(&other), ["gpt-5-codex", "gpt-5.5-pro", "llama-3.3-70b", "mistral-large-latest"]);
        // OpenAI's own list: its Responses-only models go too.
        let openai = openai_models(&answer, "https://api.openai.com/v1").unwrap();
        assert_eq!(ids(&openai), ["llama-3.3-70b", "mistral-large-latest"]);
    }

    #[test]
    fn gemini_ids_lose_their_models_prefix() {
        let answer = json!({"data": [{"id": "models/gemini-3-flash"}, {"id": "models/gemini-embedding-001"},
            {"id": "models/imagen-4"}, {"id": "gemini-3-flash"}]});
        let models = openai_models(&answer, "https://generativelanguage.googleapis.com/v1beta/openai").unwrap();
        assert_eq!(ids(&models), ["gemini-3-flash"]);
        // Elsewhere the id is taken as it is.
        let models = openai_models(&json!({"data": [{"id": "models/x"}]}), "https://llm.example/v1").unwrap();
        assert_eq!(ids(&models), ["models/x"]);
    }
}
