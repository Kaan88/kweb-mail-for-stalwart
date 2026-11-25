//! The models a service offers, for the picker in the settings. Sends no mail. Same care as
//! [`super::provider`]: the address is checked first, nothing is redirected, the key is only
//! ever a sensitive header, and the service's own words are never put into an error.

use std::time::Duration;

use serde_json::Value;
use url::Url;

use super::{
    provider::{self, effort_named, host_is, read_capped, sensitive, BodyError, ANTHROPIC_VERSION, MAX_BODY},
    settings,
};
use crate::{
    model::{AiEffort, AiModel, AiModelList, AiProvider},
    Error, Result,
};

/// The most pages of models that are followed.
const MAX_PAGES: usize = 10;
/// To reach the service.
const CONNECT: Duration = Duration::from_secs(10);
/// For the whole request, the body included.
const WHOLE: Duration = Duration::from_secs(30);

/// The levels in the order the Messages API lists them.
const LEVELS: [(&str, AiEffort); 5] = [
    ("low", AiEffort::Low),
    ("medium", AiEffort::Medium),
    ("high", AiEffort::High),
    ("xhigh", AiEffort::XHigh),
    ("max", AiEffort::Max),
];

/// What Anthropic's picker shows when the service cannot be asked. The service did not say which
/// effort levels these take, so they have none (nothing is sent until it does).
pub fn builtin() -> AiModelList {
    let models = [
        ("claude-opus-5-5", "Claude Opus 5.5"),
        ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
        ("claude-haiku-5-5", "Claude Haiku 5.5"),
        ("claude-fable-5-2", "Claude Fable 5.2"),
    ]
    .into_iter()
    .map(|(id, name)| AiModel { id: id.into(), name: name.into(), efforts: Vec::new() })
    .collect();
    AiModelList { models, from_service: false }
}

/// Why one request for a page of models failed.
enum Failed {
    /// Could not be reached, or the service is busy or broken: the built-in list will do.
    Unavailable(Error),
    /// The service answered and said no to the key (401 or 403): the user should learn of it.
    Denied(Error),
    /// The service answered something else that is not a list: another status, or an answer
    /// that cannot be read.
    Refused(Error),
}

impl Failed {
    fn into_error(self) -> Error {
        match self {
            Failed::Unavailable(e) | Failed::Denied(e) | Failed::Refused(e) => e,
        }
    }
}

/// The models of the service at `base_url` (already trimmed, without a trailing slash).
/// `key` is sent as the service's own header, when there is one.
pub async fn list(provider: AiProvider, base_url: &str, key: Option<&str>) -> Result<AiModelList> {
    let url = settings::check_base_url(base_url)?;
    let host = settings::host_label(&url);
    match provider {
        AiProvider::OpenAiCompatible => {
            let headers: Vec<(&str, String)> = key.iter().map(|k| ("authorization", format!("Bearer {k}"))).collect();
            let answer = get(&format!("{base_url}/models"), &headers, &host).await.map_err(Failed::into_error)?;
            Ok(AiModelList { models: openai_models(&answer, base_url)?, from_service: true })
        }
        AiProvider::Anthropic => {
            let Some(key) = key else { return Ok(builtin()) };
            match anthropic_models(base_url, key, &host).await {
                Ok(models) => Ok(AiModelList { models, from_service: true }),
                Err(Failed::Denied(e)) => Err(e),
                // A gateway without a list, a redirect, a page that is not JSON: the service may
                // still answer messages, so the built-in list will do.
                Err(Failed::Unavailable(_) | Failed::Refused(_)) => Ok(builtin()),
            }
        }
    }
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

/// All pages, in the order the service gives them (newest first).
async fn anthropic_models(base_url: &str, key: &str, host: &str) -> std::result::Result<Vec<AiModel>, Failed> {
    let headers = [("x-api-key", key.to_string()), ("anthropic-version", ANTHROPIC_VERSION.to_string())];
    let mut models = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut url = Url::parse(&format!("{base_url}/models")).map_err(|_| Failed::Refused(unreadable()))?;
        url.query_pairs_mut().append_pair("limit", "1000");
        if let Some(after) = &after {
            url.query_pairs_mut().append_pair("after_id", after);
        }
        let page = get(url.as_str(), &headers, host).await?;
        let data = page["data"].as_array().ok_or_else(|| Failed::Refused(unreadable()))?;
        models.extend(data.iter().filter(|m| usable(m)).filter_map(anthropic_model));
        match (page["has_more"].as_bool(), page["last_id"].as_str()) {
            (Some(true), Some(last)) => after = Some(last.to_string()),
            _ => break,
        }
    }
    Ok(models)
}

/// Whether a model of Anthropic's list may still be chosen: the list also has `deprecated`
/// models, which are closed to new users and answer them with an error, and `retired` ones.
fn usable(m: &Value) -> bool {
    !matches!(m["lifecycle"].as_str(), Some("deprecated" | "retired"))
}

fn anthropic_model(m: &Value) -> Option<AiModel> {
    let id = m["id"].as_str()?;
    let name = m["display_name"].as_str().filter(|n| !n.is_empty()).unwrap_or(id);
    let effort = &m["capabilities"]["effort"];
    let efforts = if effort["supported"].as_bool() == Some(true) {
        LEVELS.iter().filter(|(word, _)| effort[*word]["supported"].as_bool() == Some(true)).map(|(_, e)| *e).collect()
    } else {
        Vec::new()
    };
    Some(AiModel { id: id.into(), name: name.into(), efforts })
}

/// One GET, the answer as JSON.
async fn get(url: &str, headers: &[(&str, String)], host: &str) -> std::result::Result<Value, Failed> {
    let unavailable = |msg: String| Failed::Unavailable(Error::temporary(msg));
    let client = provider::client(CONNECT, Some(WHOLE)).map_err(Failed::Unavailable)?;
    let mut request = client.get(url).header("accept", "application/json");
    for (name, value) in headers {
        request = request.header(*name, sensitive(value).map_err(Failed::Refused)?);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(e) if e.is_timeout() => return Err(unavailable("the service did not answer in time".into())),
        Err(_) => return Err(unavailable(format!("could not reach the service at {host}"))),
    };
    let status = response.status().as_u16();
    if !response.status().is_success() {
        let error = provider::refused(response, WHOLE).await;
        return Err(match status {
            429 | 500..=599 => Failed::Unavailable(error),
            401 | 403 => Failed::Denied(error),
            _ => Failed::Refused(error),
        });
    }
    let body = match read_capped(response, MAX_BODY, WHOLE).await {
        Ok(body) => body,
        Err(BodyError::TooLarge) => return Err(Failed::Refused(Error::temporary("the list of models is too long"))),
        Err(BodyError::Broken) => return Err(unavailable("the service stopped answering".into())),
    };
    serde_json::from_slice(&body).map_err(|_| Failed::Refused(unreadable()))
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

    #[test]
    fn deprecated_and_retired_anthropic_models_are_not_offered() {
        assert!(usable(&json!({"id": "a", "lifecycle": "active"})));
        assert!(usable(&json!({"id": "a"})));
        assert!(!usable(&json!({"id": "a", "lifecycle": "deprecated"})));
        assert!(!usable(&json!({"id": "a", "lifecycle": "retired"})));
    }

    #[test]
    fn an_anthropic_model_lists_the_levels_it_supports_lowest_first() {
        let m = json!({"id": "m", "display_name": "M", "capabilities": {"effort": {
            "supported": true, "low": {"supported": true}, "medium": {"supported": false},
            "high": {"supported": true}, "max": {"supported": true}}}});
        let model = anthropic_model(&m).unwrap();
        assert_eq!(model.efforts, [AiEffort::Low, AiEffort::High, AiEffort::Max]);
        assert_eq!(model.name, "M");
        let off = json!({"id": "n", "capabilities": {"effort": {"supported": false, "low": {"supported": true}}}});
        let model = anthropic_model(&off).unwrap();
        assert!(model.efforts.is_empty());
        assert_eq!(model.name, "n");
        assert!(anthropic_model(&json!({"display_name": "x"})).is_none());
    }

    #[test]
    fn the_built_in_list_has_four_models_and_no_levels() {
        let list = builtin();
        assert!(!list.from_service);
        assert_eq!(list.models.len(), 4);
        assert!(list.models.iter().all(|m| m.efforts.is_empty()));
        assert_eq!(list.models[3].id, "claude-fable-5-2");
    }
}
