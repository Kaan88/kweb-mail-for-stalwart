//! `kweb ai`: the assistant from the command line. The key is read from `KWEB_AI_KEY` and from
//! nowhere else (an argument would end up in the shell history and the process list), and is
//! never printed.

use std::{
    io::{self, Write},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use kweb_core::{
    assistant::settings as rules,
    ffi::{AiListener, Core},
    model::{AiEffort, AiModelList, AiProvider, AiSettings, AiTask},
};

use crate::{args::Ai, commands::printable};

/// The environment variable that holds the key.
pub const KEY_VARIABLE: &str = "KWEB_AI_KEY";

pub fn run(core: &Core, what: Ai, env_key: Option<String>) -> Result<(), String> {
    match what {
        Ai::Status => {
            println!("{}", status_text(&core.ai_settings().map_err(|e| e.to_string())?));
            Ok(())
        }
        Ai::Set { provider, base_url, model, effort, yes } => {
            // What is stored for this service, whichever is in use: its effort and its consent.
            let current = core.ai_service(provider).map_err(|e| e.to_string())?;
            match plan_set(&current, provider, &base_url, &model, effort, yes, env_key)? {
                Outcome::NeedsYes(notice) => Err(notice),
                Outcome::Store { settings, key } => {
                    // A change of service clears the yes, so it is given by a second save of the
                    // same values (what the user agreed to with `--yes`).
                    core.set_ai_settings(settings.clone(), key.clone()).map_err(|e| e.to_string())?;
                    let stored = core.set_ai_settings(settings, None).map_err(|e| e.to_string())?;
                    if current.has_key && !stored.has_key && key.is_none() {
                        println!(
                            "The stored key was removed because the service changed; set {KEY_VARIABLE} to store one."
                        );
                    }
                    println!("{}", status_text(&stored));
                    Ok(())
                }
            }
        }
        Ai::Models { provider } => {
            let in_use = core.ai_settings().map_err(|e| e.to_string())?;
            let service = core.ai_service(provider.unwrap_or(in_use.provider)).map_err(|e| e.to_string())?;
            if service.base_url.is_empty() {
                return Err("this service has no address yet: run `kweb ai set` first".into());
            }
            let key = env_key.filter(|k| !k.trim().is_empty());
            println!("{}", models_text(&core.ai_models(service, key).map_err(|e| e.to_string())?));
            Ok(())
        }
        Ai::Summary { id } => summary(core, id),
    }
}

/// A fresh name for a run: std has no UUID, and the `uuid` crate is not a dependency, so this
/// is the time, the process and a counter (two runs of one process never share it).
fn run_id() -> String {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    format!("cli-{nanos:x}-{}-{}", std::process::id(), COUNT.fetch_add(1, Ordering::Relaxed))
}

fn effort_name(e: AiEffort) -> &'static str {
    match e {
        AiEffort::Default => "default",
        AiEffort::Low => "low",
        AiEffort::Medium => "medium",
        AiEffort::High => "high",
        AiEffort::XHigh => "xhigh",
        AiEffort::Max => "max",
    }
}

/// One line per model (id, name, efforts), after a line that says where the list is from.
/// The names are the service's words: control characters are left out.
pub fn models_text(list: &AiModelList) -> String {
    let mut out = String::from(if list.from_service {
        "Models from the service:"
    } else {
        "Built-in list (the service was not asked, or could not be):"
    });
    for m in &list.models {
        let efforts: Vec<&str> = m.efforts.iter().map(|e| effort_name(*e)).collect();
        let efforts = if efforts.is_empty() { "-".to_string() } else { efforts.join(",") };
        out.push_str(&format!("\n{}\t{}\t{efforts}", printable(&m.id), printable(&m.name)));
    }
    out
}

fn summary(core: &Core, id: String) -> Result<(), String> {
    ready(&core.ai_settings().map_err(|e| e.to_string())?)?;
    let task = AiTask::SummarizeMessage { message_id: id };
    let preview = core.ai_preview(task.clone()).map_err(|e| e.to_string())?;
    let cut = if preview.is_cut { ", cut to the limits" } else { "" };
    eprintln!("Sending {} characters of 1 message{cut} to {} ({}).", preview.characters, preview.host, preview.model);
    core.ai_run(task, run_id(), Arc::new(Printer)).map_err(|e| e.to_string())?;
    println!();
    Ok(())
}

/// Prints the answer as it streams. The answer is shaped by mail from strangers, so control
/// characters (a terminal escape sequence) are left out.
struct Printer;

impl AiListener for Printer {
    fn text(&self, piece: String) {
        let mut out = io::stdout().lock();
        let _ = write!(out, "{}", printable(&piece));
        let _ = out.flush();
    }
}

/// The assistant must be on and agreed to before a task can be started.
fn ready(s: &AiSettings) -> Result<(), String> {
    if s.enabled && s.consented {
        Ok(())
    } else {
        Err("the assistant is not set up: run `kweb ai set <provider> <base-url> <model> --yes`".into())
    }
}

fn provider_name(p: AiProvider) -> &'static str {
    match p {
        AiProvider::OpenAiCompatible => "OpenAI-compatible",
        AiProvider::Anthropic => "Anthropic",
    }
}

/// The settings for the user to read. The key is never in them: only whether there is one.
pub fn status_text(s: &AiSettings) -> String {
    let yes = |b: bool| if b { "yes" } else { "no" };
    let or_unset = |v: &str| if v.is_empty() { "(not set)".to_string() } else { v.to_string() };
    format!(
        "Assistant: {}\nService:   {}, {}\nModel:     {}\nEffort:    {}\nKey:       {}\nAgreed:    {}\nAllowed:   summaries {}, drafts {}, rewrites {}",
        if s.enabled { "on" } else { "off" },
        provider_name(s.provider),
        or_unset(&s.base_url),
        or_unset(&s.model),
        effort_name(s.effort),
        if s.has_key { "stored" } else { "none" },
        yes(s.consented),
        yes(s.allow_summaries),
        yes(s.allow_drafts),
        yes(s.allow_rewrites),
    )
}

/// What the user agrees to by adding `--yes`.
pub fn consent_text(provider: AiProvider, host: &str, model: &str) -> String {
    format!(
        "Mail you ask the assistant about is sent, as text, to {host} ({}, model {model}).\n\
         Nothing is sent until you start a task, and then only the mail that task needs.\n\
         To agree, run the same command again with --yes.",
        provider_name(provider)
    )
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Nothing is stored; this says what the user would agree to.
    NeedsYes(String),
    Store {
        settings: AiSettings,
        /// As `Core::set_ai_settings` takes it.
        key: Option<String>,
    },
}

/// What `kweb ai set` does, from what is stored for that service (`current`, as
/// `Core::ai_service` gives it). A key stored for this service is removed by the core when its
/// address changes and no new key is given.
pub fn plan_set(
    current: &AiSettings,
    provider: AiProvider,
    base_url: &str,
    model: &str,
    effort: Option<AiEffort>,
    yes: bool,
    env_key: Option<String>,
) -> Result<Outcome, String> {
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    let model = model.trim().to_string();
    let url = rules::check_base_url(&base_url).map_err(|e| e.to_string())?;
    if model.is_empty() {
        return Err("the model name is empty".into());
    }
    if !yes {
        return Ok(Outcome::NeedsYes(consent_text(provider, &rules::host_label(&url), &model)));
    }
    let key = env_key.filter(|k| !k.trim().is_empty());
    let effort = effort.unwrap_or(current.effort);
    let settings = AiSettings { enabled: true, provider, base_url, model, effort, consented: true, ..current.clone() };
    Ok(Outcome::Store { settings, key })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current() -> AiSettings {
        AiSettings {
            enabled: false,
            provider: AiProvider::OpenAiCompatible,
            base_url: String::new(),
            model: String::new(),
            effort: AiEffort::Default,
            has_key: false,
            consented: false,
            allow_summaries: true,
            allow_drafts: false,
            allow_rewrites: true,
        }
    }

    #[test]
    fn status_shows_whether_there_is_a_key_and_never_the_key() {
        let s = AiSettings { has_key: true, ..current() };
        let text = status_text(&s);
        assert!(text.contains("Key:       stored") && text.contains("Assistant: off"));
        assert!(text.contains("Service:   OpenAI-compatible, (not set)"));
        assert!(text.contains("summaries yes, drafts no, rewrites yes"));
        assert!(status_text(&current()).contains("Key:       none"));
    }

    #[test]
    fn without_yes_nothing_is_stored_and_the_notice_names_the_host_and_model() {
        let plan = plan_set(
            &current(),
            AiProvider::Anthropic,
            "https://api.example.com/v1/",
            "m-1",
            None,
            false,
            Some("k".into()),
        );
        let Ok(Outcome::NeedsYes(notice)) = plan else { panic!("{plan:?}") };
        assert!(notice.contains("api.example.com (Anthropic, model m-1)") && notice.contains("--yes"));
        assert!(!notice.contains("k\n"));
    }

    #[test]
    fn with_yes_it_enables_and_agrees_and_keeps_the_other_switches() {
        let plan = plan_set(
            &current(),
            AiProvider::OpenAiCompatible,
            " http://127.0.0.1:11434/v1/ ",
            " llama ",
            None,
            true,
            None,
        );
        let Ok(Outcome::Store { settings, key }) = plan else { panic!("{plan:?}") };
        assert!(settings.enabled && settings.consented);
        assert_eq!((settings.base_url.as_str(), settings.model.as_str()), ("http://127.0.0.1:11434/v1", "llama"));
        assert!(!settings.allow_drafts, "the other switches are kept");
        assert_eq!(key, None);
    }

    #[test]
    fn effort_is_taken_from_the_option_or_kept_from_what_is_stored() {
        let stored = AiSettings { effort: AiEffort::High, ..current() };
        let effort = |given: Option<AiEffort>| match plan_set(
            &stored,
            AiProvider::Anthropic,
            "https://a.example/v1",
            "m",
            given,
            true,
            None,
        ) {
            Ok(Outcome::Store { settings, .. }) => settings.effort,
            other => panic!("{other:?}"),
        };
        assert_eq!(effort(None), AiEffort::High);
        assert_eq!(effort(Some(AiEffort::Max)), AiEffort::Max);
        assert_eq!(effort(Some(AiEffort::Default)), AiEffort::Default);
        assert!(status_text(&stored).contains("Effort:    high"));
    }

    #[test]
    fn the_model_list_says_where_it_is_from_and_never_shows_control_characters() {
        use kweb_core::model::AiModel;
        let model = |id: &str, name: &str, efforts: &[AiEffort]| AiModel {
            id: id.into(),
            name: name.into(),
            efforts: efforts.to_vec(),
        };
        let list = AiModelList {
            models: vec![model("a", "A\u{1b}[31m", &[AiEffort::Low, AiEffort::XHigh]), model("b", "b", &[])],
            from_service: true,
        };
        let text = models_text(&list);
        assert_eq!(text, "Models from the service:\na\tA[31m\tlow,xhigh\nb\tb\t-");
        assert!(models_text(&AiModelList { from_service: false, ..list }).starts_with("Built-in list"));
    }

    #[test]
    fn run_ids_are_never_repeated() {
        let ids: std::collections::HashSet<String> = (0..100).map(|_| run_id()).collect();
        assert_eq!(ids.len(), 100);
    }

    #[test]
    fn the_key_comes_from_the_environment_only_and_an_empty_one_is_no_key() {
        let with = |env: Option<&str>| match plan_set(
            &current(),
            AiProvider::Anthropic,
            "https://a.example/v1",
            "m",
            None,
            true,
            env.map(String::from),
        ) {
            Ok(Outcome::Store { key, .. }) => key,
            other => panic!("{other:?}"),
        };
        assert_eq!(with(Some("sk-1")), Some("sk-1".into()));
        assert_eq!(with(Some("  ")), None);
        assert_eq!(with(None), None);
    }

    #[test]
    fn bad_addresses_and_models_are_refused_before_anything_else() {
        for (url, model) in [("http://api.example.com/v1", "m"), ("nonsense", "m"), ("https://a.example/v1", "  ")] {
            for yes in [false, true] {
                assert!(
                    plan_set(&current(), AiProvider::Anthropic, url, model, None, yes, None).is_err(),
                    "{url} {model}"
                );
            }
        }
    }

    #[test]
    fn a_task_needs_the_assistant_on_and_agreed_to() {
        assert!(ready(&current()).is_err());
        assert!(ready(&AiSettings { enabled: true, ..current() }).is_err());
        assert!(ready(&AiSettings { enabled: true, consented: true, ..current() }).is_ok());
    }
}
