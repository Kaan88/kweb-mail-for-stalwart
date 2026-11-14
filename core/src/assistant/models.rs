//! The models a service offers, for the picker in the settings. Sends no mail. Same care as
//! [`super::provider`]: the address is checked first, nothing is redirected, the key is only
//! ever a sensitive header, and the service's own words are never put into an error.

use std::time::Duration;

use serde_json::Value;
use url::Url;

use super::{
    provider::{self, read_capped, sensitive, status_error, BodyError, ANTHROPIC_VERSION, MAX_BODY},
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

/// What Anthropic's picker shows when the service cannot be asked.
pub fn builtin() -> AiModelList {
    let efforts: Vec<AiEffort> = LEVELS.iter().map(|(_, level)| *level).collect();
    let models = [
        ("claude-opus-5-5", "Claude Opus 5.5"),
        ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
        ("claude-haiku-5-5", "Claude Haiku 5.5"),
        ("claude-fable-5-2", "Claude Fable 5.2"),
    ]
    .into_iter()
    .map(|(id, name)| AiModel { id: id.into(), name: name.into(), efforts: efforts.clone() })
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
            Ok(AiModelList { models: openai_models(&answer)?, from_service: true })
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

/// `{"data":[{"id":..}]}`, by id.
fn openai_models(answer: &Value) -> Result<Vec<AiModel>> {
    let data = answer["data"].as_array().ok_or_else(unreadable)?;
    let mut models: Vec<AiModel> = data
        .iter()
        .filter_map(|m| m["id"].as_str())
        .map(|id| AiModel { id: id.into(), name: id.into(), efforts: Vec::new() })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
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
        models.extend(data.iter().filter_map(anthropic_model));
        match (page["has_more"].as_bool(), page["last_id"].as_str()) {
            (Some(true), Some(last)) => after = Some(last.to_string()),
            _ => break,
        }
    }
    Ok(models)
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
        let error = status_error(status);
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

    #[test]
    fn openai_models_are_sorted_by_id_and_have_no_efforts() {
        let answer = json!({"data": [{"id": "b"}, {"id": "a", "owned_by": "x"}, {"nope": 1}]});
        let models = openai_models(&answer).unwrap();
        assert_eq!(models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert!(models.iter().all(|m| m.name == m.id && m.efforts.is_empty()));
        assert!(matches!(openai_models(&json!({"models": []})), Err(Error::Temporary { .. })));
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
    fn the_built_in_list_has_four_models_with_every_level() {
        let list = builtin();
        assert!(!list.from_service);
        assert_eq!(list.models.len(), 4);
        assert!(list.models.iter().all(|m| m.efforts.len() == 5));
        assert_eq!(list.models[3].id, "claude-fable-5-2");
    }
}
