//! Command line parsing, by hand: the commands are few and take at most one argument.

use std::path::PathBuf;

use kweb_core::model::AiProvider;

pub const USAGE: &str = "\
kweb: a developer tool that drives kweb-core against a Stalwart server.

Usage:
  kweb sign-in <server-url>   sign in through the browser
  kweb status                 show the account
  kweb inbox [-n N]           sync, then show the newest N Inbox messages (default 20)
  kweb mailboxes              sync, then list mailboxes with totals
  kweb show <message-id>      show a message: headers, text, attachments
  kweb search <query>         search all mail on the server (the cache when offline)
  kweb watch                  follow live changes until Ctrl+C
  kweb read <message-id>      mark a message read
  kweb unread <message-id>    mark a message unread
  kweb archive <message-id>   move a message to Archive
  kweb delete <message-id>    move a message to Trash
  kweb destroy <message-id>   delete a message for good
  kweb agenda [days]          show events and tasks for the next days (default 7), in $TZ or UTC
  kweb calendars              list calendars
  kweb identities             list the addresses you can send as
  kweb quota                  show storage use and limits
  kweb contacts [query]       list contacts, or those matching the words of the query
  kweb away                   show the away message
  kweb away on <text>         switch it on from now until switched off, with this text
  kweb away off               switch it off
  kweb send <to> <subject> <text>
                              send a plain message now (no undo delay)
  kweb ai status              show how the AI assistant is set up (never the key)
  kweb ai set <provider> <base-url> <model> [--yes]
                              set it up; provider is openai or anthropic. The key is read
                              from $KWEB_AI_KEY. Without --yes it only says what you agree to
  kweb ai summary <message-id>
                              summarise a message, printed as it streams
  kweb sign-out               revoke the grant and wipe the cache
  kweb help                   this text

Data lives in $KWEB_HOME, else %APPDATA%\\kweb-mail (Windows), else
$XDG_CONFIG_HOME/kweb-mail or ~/.config/kweb-mail: cache.db and secrets.json.

WARNING: secrets.json holds the OAuth refresh token in plain text. kweb is a
developer tool; do not use it with an account you cannot afford to lose access to.";

pub const DEFAULT_INBOX_COUNT: u32 = 20;
pub const DEFAULT_AGENDA_DAYS: u32 = 7;
/// The longest span the server answers for (a year).
pub const MAX_AGENDA_DAYS: u32 = 365;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Help,
    SignIn { server_url: String },
    Status,
    Inbox { count: u32 },
    Mailboxes,
    Show { id: String },
    Search { query: String },
    Watch,
    Read { id: String },
    Unread { id: String },
    Archive { id: String },
    Delete { id: String },
    Destroy { id: String },
    Agenda { days: u32 },
    Calendars,
    Identities,
    Quota,
    Contacts { query: String },
    Away(Away),
    Send { to: String, subject: String, text: String },
    Ai(Ai),
    SignOut,
}

/// What `kweb ai` does.
#[derive(Debug, Clone, PartialEq)]
pub enum Ai {
    Status,
    Set { provider: AiProvider, base_url: String, model: String, yes: bool },
    Summary { id: String },
}

/// What `kweb away` does.
#[derive(Debug, Clone, PartialEq)]
pub enum Away {
    Show,
    On { text: String },
    Off,
}

/// `args` is everything after the program name. The error says what is wrong.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let Some((name, rest)) = args.split_first() else {
        return Ok(Command::Help);
    };
    match name.as_str() {
        "help" | "--help" | "-h" => no_args(rest, Command::Help),
        "status" => no_args(rest, Command::Status),
        "mailboxes" => no_args(rest, Command::Mailboxes),
        "watch" => no_args(rest, Command::Watch),
        "sign-out" => no_args(rest, Command::SignOut),
        "calendars" => no_args(rest, Command::Calendars),
        "agenda" => agenda(rest),
        "identities" => no_args(rest, Command::Identities),
        "quota" => no_args(rest, Command::Quota),
        "contacts" => Ok(Command::Contacts { query: rest.join(" ") }),
        "away" => away(rest),
        "send" => send(rest),
        "ai" => ai(rest).map(Command::Ai),
        "sign-in" => one_arg(name, "server-url", rest).map(|server_url| Command::SignIn { server_url }),
        "show" => one_arg(name, "message-id", rest).map(|id| Command::Show { id }),
        "search" => search(rest),
        "read" => one_arg(name, "message-id", rest).map(|id| Command::Read { id }),
        "unread" => one_arg(name, "message-id", rest).map(|id| Command::Unread { id }),
        "archive" => one_arg(name, "message-id", rest).map(|id| Command::Archive { id }),
        "delete" => one_arg(name, "message-id", rest).map(|id| Command::Delete { id }),
        "destroy" => one_arg(name, "message-id", rest).map(|id| Command::Destroy { id }),
        "inbox" => inbox(rest),
        other => Err(format!("unknown command '{other}'")),
    }
}

fn no_args(rest: &[String], command: Command) -> Result<Command, String> {
    match rest.first() {
        None => Ok(command),
        Some(extra) => Err(format!("unexpected argument '{extra}'")),
    }
}

fn one_arg(command: &str, what: &str, rest: &[String]) -> Result<String, String> {
    match rest {
        [one] => Ok(one.clone()),
        [] => Err(format!("{command} needs a <{what}>")),
        [_, extra, ..] => Err(format!("unexpected argument '{extra}'")),
    }
}

/// The words after `search` are one query, so quotes are optional.
fn search(rest: &[String]) -> Result<Command, String> {
    let query = rest.join(" ");
    if query.trim().is_empty() {
        return Err("search needs a <query>".into());
    }
    Ok(Command::Search { query })
}

fn away(rest: &[String]) -> Result<Command, String> {
    match rest {
        [] => Ok(Command::Away(Away::Show)),
        [flag] if flag == "off" => Ok(Command::Away(Away::Off)),
        [flag, _extra, ..] if flag == "off" => Err(format!("unexpected argument '{}'", rest[1])),
        [flag, words @ ..] if flag == "on" => match words.join(" ") {
            text if text.trim().is_empty() => Err("away on needs the <text> to send".into()),
            text => Ok(Command::Away(Away::On { text })),
        },
        [other, ..] => Err(format!("away takes on <text>, off, or nothing, not '{other}'")),
    }
}

fn send(rest: &[String]) -> Result<Command, String> {
    match rest {
        [to, subject, text] if !to.trim().is_empty() => {
            Ok(Command::Send { to: to.clone(), subject: subject.clone(), text: text.clone() })
        }
        [_, _, _] => Err("send needs a <to> address".into()),
        [_, _, _, extra, ..] => Err(format!("unexpected argument '{extra}'")),
        _ => Err("send needs <to> <subject> <text>".into()),
    }
}

fn ai(rest: &[String]) -> Result<Ai, String> {
    match rest.split_first() {
        None => Err("ai needs status, set or summary".into()),
        Some((sub, rest)) => match sub.as_str() {
            "status" => no_args(rest, Command::Status).map(|_| Ai::Status),
            "summary" => one_arg("ai summary", "message-id", rest).map(|id| Ai::Summary { id }),
            "set" => ai_set(rest),
            other => Err(format!("ai takes status, set or summary, not '{other}'")),
        },
    }
}

/// The key is never an argument: it is read from the environment (see `kweb help`).
fn ai_set(rest: &[String]) -> Result<Ai, String> {
    let yes = rest.iter().any(|a| a == "--yes");
    let words: Vec<&String> = rest.iter().filter(|a| *a != "--yes").collect();
    if let Some(flag) = words.iter().find(|a| a.starts_with("--")) {
        return Err(format!("unknown option '{flag}'"));
    }
    let [provider, base_url, model] = words[..] else {
        return Err("ai set needs <provider> <base-url> <model>".into());
    };
    let provider = match provider.to_ascii_lowercase().as_str() {
        "openai" | "openai-compatible" => AiProvider::OpenAiCompatible,
        "anthropic" => AiProvider::Anthropic,
        other => return Err(format!("the provider is openai or anthropic, not '{other}'")),
    };
    Ok(Ai::Set { provider, base_url: base_url.clone(), model: model.clone(), yes })
}

fn agenda(rest: &[String]) -> Result<Command, String> {
    match rest {
        [] => Ok(Command::Agenda { days: DEFAULT_AGENDA_DAYS }),
        [n] => match n.parse::<u32>() {
            Ok(days) if (1..=MAX_AGENDA_DAYS).contains(&days) => Ok(Command::Agenda { days }),
            _ => Err(format!("agenda needs a number of days from 1 to {MAX_AGENDA_DAYS}, not '{n}'")),
        },
        [_, extra, ..] => Err(format!("unexpected argument '{extra}'")),
    }
}

fn inbox(rest: &[String]) -> Result<Command, String> {
    match rest {
        [] => Ok(Command::Inbox { count: DEFAULT_INBOX_COUNT }),
        [flag, n] if flag == "-n" => match n.parse::<u32>() {
            Ok(count) if count > 0 => Ok(Command::Inbox { count }),
            _ => Err(format!("-n needs a positive number, not '{n}'")),
        },
        [flag] if flag == "-n" => Err("-n needs a number".into()),
        [other, ..] => Err(format!("unexpected argument '{other}'")),
    }
}

/// Where `cache.db` and `secrets.json` live. `get` reads an environment variable (a parameter
/// so the tests need not touch the real environment).
pub fn data_dir(get: impl Fn(&str) -> Option<String>, windows: bool) -> Option<PathBuf> {
    let set = |name: &str| get(name).filter(|v| !v.is_empty());
    if let Some(home) = set("KWEB_HOME") {
        return Some(PathBuf::from(home));
    }
    if windows {
        return set("APPDATA").map(|dir| PathBuf::from(dir).join("kweb-mail"));
    }
    if let Some(xdg) = set("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("kweb-mail"));
    }
    set("HOME").map(|home| PathBuf::from(home).join(".config").join("kweb-mail"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn no_arguments_and_help_show_usage() {
        assert_eq!(p(&[]), Ok(Command::Help));
        assert_eq!(p(&["help"]), Ok(Command::Help));
        assert_eq!(p(&["--help"]), Ok(Command::Help));
    }

    #[test]
    fn simple_commands() {
        assert_eq!(p(&["status"]), Ok(Command::Status));
        assert_eq!(p(&["mailboxes"]), Ok(Command::Mailboxes));
        assert_eq!(p(&["watch"]), Ok(Command::Watch));
        assert_eq!(p(&["sign-out"]), Ok(Command::SignOut));
    }

    #[test]
    fn commands_with_one_argument() {
        assert_eq!(
            p(&["sign-in", "https://mail.example"]),
            Ok(Command::SignIn { server_url: "https://mail.example".into() })
        );
        assert_eq!(p(&["show", "e1"]), Ok(Command::Show { id: "e1".into() }));
        assert_eq!(p(&["read", "e1"]), Ok(Command::Read { id: "e1".into() }));
        assert_eq!(p(&["unread", "e1"]), Ok(Command::Unread { id: "e1".into() }));
        assert_eq!(p(&["archive", "e1"]), Ok(Command::Archive { id: "e1".into() }));
        assert_eq!(p(&["delete", "e1"]), Ok(Command::Delete { id: "e1".into() }));
        assert_eq!(p(&["destroy", "e1"]), Ok(Command::Destroy { id: "e1".into() }));
    }

    #[test]
    fn missing_and_extra_arguments_are_errors() {
        assert!(p(&["read"]).is_err());
        assert!(p(&["read", "a", "b"]).is_err());
        assert!(p(&["show"]).is_err());
        assert!(p(&["show", "a", "b"]).is_err());
        assert!(p(&["status", "x"]).is_err());
        assert!(p(&["sign-in"]).is_err());
    }

    #[test]
    fn ai_commands() {
        assert_eq!(p(&["ai", "status"]), Ok(Command::Ai(Ai::Status)));
        assert_eq!(p(&["ai", "summary", "e1"]), Ok(Command::Ai(Ai::Summary { id: "e1".into() })));
        let set = |provider, yes| {
            Ok(Command::Ai(Ai::Set { provider, base_url: "https://a.example/v1".into(), model: "m".into(), yes }))
        };
        assert_eq!(p(&["ai", "set", "openai", "https://a.example/v1", "m"]), set(AiProvider::OpenAiCompatible, false));
        assert_eq!(
            p(&["ai", "set", "Anthropic", "https://a.example/v1", "m", "--yes"]),
            set(AiProvider::Anthropic, true)
        );
        assert_eq!(
            p(&["ai", "set", "--yes", "openai", "https://a.example/v1", "m"]),
            set(AiProvider::OpenAiCompatible, true)
        );
    }

    #[test]
    fn ai_mistakes_are_errors_and_a_key_is_never_an_argument() {
        for bad in [
            &["ai"][..],
            &["ai", "nope"],
            &["ai", "status", "x"],
            &["ai", "summary"],
            &["ai", "summary", "a", "b"],
            &["ai", "set"],
            &["ai", "set", "openai", "https://a.example/v1"],
            &["ai", "set", "gemini", "https://a.example/v1", "m"],
            &["ai", "set", "openai", "https://a.example/v1", "m", "extra"],
            &["ai", "set", "openai", "https://a.example/v1", "m", "--key", "sk-1"],
            &["ai", "set", "openai", "https://a.example/v1", "m", "--key=sk-1"],
        ] {
            assert!(p(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn search_takes_the_rest_of_the_line_as_one_query() {
        assert_eq!(p(&["search", "invoice"]), Ok(Command::Search { query: "invoice".into() }));
        assert_eq!(p(&["search", "big", "invoice"]), Ok(Command::Search { query: "big invoice".into() }));
        assert!(p(&["search"]).is_err());
        assert!(p(&["search", " "]).is_err());
    }

    #[test]
    fn inbox_count() {
        assert_eq!(p(&["inbox"]), Ok(Command::Inbox { count: 20 }));
        assert_eq!(p(&["inbox", "-n", "5"]), Ok(Command::Inbox { count: 5 }));
        assert!(p(&["inbox", "-n"]).is_err());
        assert!(p(&["inbox", "-n", "0"]).is_err());
        assert!(p(&["inbox", "-n", "x"]).is_err());
        assert!(p(&["inbox", "5"]).is_err());
    }

    #[test]
    fn identities_quota_and_send() {
        assert_eq!(p(&["identities"]), Ok(Command::Identities));
        assert_eq!(p(&["quota"]), Ok(Command::Quota));
        assert!(p(&["quota", "x"]).is_err());
        assert_eq!(
            p(&["send", "bo@example.test", "Hi", "Hello Bo"]),
            Ok(Command::Send { to: "bo@example.test".into(), subject: "Hi".into(), text: "Hello Bo".into() })
        );
        assert!(p(&["send", "bo@example.test", "Hi"]).is_err());
        assert!(p(&["send", "a", "b", "c", "d"]).is_err());
        assert!(p(&["send", " ", "b", "c"]).is_err());
    }

    #[test]
    fn contacts_take_the_rest_of_the_line_as_one_query() {
        assert_eq!(p(&["contacts"]), Ok(Command::Contacts { query: String::new() }));
        assert_eq!(p(&["contacts", "ann"]), Ok(Command::Contacts { query: "ann".into() }));
        assert_eq!(p(&["contacts", "ann", "lee"]), Ok(Command::Contacts { query: "ann lee".into() }));
    }

    #[test]
    fn away_shows_switches_on_with_text_and_switches_off() {
        assert_eq!(p(&["away"]), Ok(Command::Away(Away::Show)));
        assert_eq!(p(&["away", "off"]), Ok(Command::Away(Away::Off)));
        assert_eq!(
            p(&["away", "on", "Back", "on", "Monday"]),
            Ok(Command::Away(Away::On { text: "Back on Monday".into() }))
        );
        assert!(p(&["away", "on"]).is_err());
        assert!(p(&["away", "on", " "]).is_err());
        assert!(p(&["away", "off", "now"]).unwrap_err().contains("now"));
        assert!(p(&["away", "maybe"]).unwrap_err().contains("maybe"));
    }

    #[test]
    fn agenda_days_and_calendars() {
        assert_eq!(p(&["agenda"]), Ok(Command::Agenda { days: 7 }));
        assert_eq!(p(&["agenda", "30"]), Ok(Command::Agenda { days: 30 }));
        assert_eq!(p(&["agenda", "365"]), Ok(Command::Agenda { days: 365 }));
        assert!(p(&["agenda", "0"]).is_err());
        assert!(p(&["agenda", "366"]).is_err());
        assert!(p(&["agenda", "x"]).is_err());
        assert!(p(&["agenda", "1", "2"]).is_err());
        assert_eq!(p(&["calendars"]), Ok(Command::Calendars));
        assert!(p(&["calendars", "x"]).is_err());
    }

    #[test]
    fn unknown_command() {
        assert!(p(&["frobnicate"]).unwrap_err().contains("frobnicate"));
    }

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string())
    }

    #[test]
    fn data_dir_prefers_kweb_home() {
        let got = data_dir(env(&[("KWEB_HOME", "/x"), ("APPDATA", "/a"), ("HOME", "/h")]), true);
        assert_eq!(got, Some(PathBuf::from("/x")));
    }

    #[test]
    fn data_dir_per_platform() {
        assert_eq!(data_dir(env(&[("APPDATA", "/a")]), true), Some(PathBuf::from("/a").join("kweb-mail")));
        assert_eq!(
            data_dir(env(&[("XDG_CONFIG_HOME", "/c"), ("HOME", "/h")]), false),
            Some(PathBuf::from("/c").join("kweb-mail"))
        );
        assert_eq!(
            data_dir(env(&[("HOME", "/h")]), false),
            Some(PathBuf::from("/h").join(".config").join("kweb-mail"))
        );
        assert_eq!(data_dir(env(&[]), false), None);
        assert_eq!(data_dir(env(&[("KWEB_HOME", "")]), true), None);
    }
}
