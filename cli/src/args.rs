//! Command line parsing, by hand: the commands are few and take at most one argument.

use std::path::PathBuf;

pub const USAGE: &str = "\
kweb: a developer tool that drives kweb-core against a Stalwart server.

Usage:
  kweb sign-in <server-url>   sign in through the browser
  kweb status                 show the account
  kweb inbox [-n N]           sync, then show the newest N Inbox messages (default 20)
  kweb mailboxes              sync, then list mailboxes with totals
  kweb watch                  follow live changes until Ctrl+C
  kweb read <message-id>      mark a message read
  kweb unread <message-id>    mark a message unread
  kweb archive <message-id>   move a message to Archive
  kweb delete <message-id>    move a message to Trash
  kweb destroy <message-id>   delete a message for good
  kweb sign-out               revoke the grant and wipe the cache
  kweb help                   this text

Data lives in $KWEB_HOME, else %APPDATA%\\kweb-mail (Windows), else
$XDG_CONFIG_HOME/kweb-mail or ~/.config/kweb-mail: cache.db and secrets.json.

WARNING: secrets.json holds the OAuth refresh token in plain text. kweb is a
developer tool; do not use it with an account you cannot afford to lose access to.";

pub const DEFAULT_INBOX_COUNT: u32 = 20;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Help,
    SignIn { server_url: String },
    Status,
    Inbox { count: u32 },
    Mailboxes,
    Watch,
    Read { id: String },
    Unread { id: String },
    Archive { id: String },
    Delete { id: String },
    Destroy { id: String },
    SignOut,
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
        "sign-in" => one_arg(name, "server-url", rest).map(|server_url| Command::SignIn { server_url }),
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
        assert!(p(&["status", "x"]).is_err());
        assert!(p(&["sign-in"]).is_err());
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
