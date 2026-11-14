//! Command line parsing, by hand: the commands are few and take at most one argument.

use std::path::PathBuf;

use kweb_core::model::{AiEffort, AiProvider, ShareKind, ShareLevel};

pub const USAGE: &str = "\
kweb: a developer tool that drives kweb-core against a Stalwart server.

Usage:
  kweb sign-in <server-url>   sign in through the browser
  kweb status                 show the account
  kweb inbox [-n N]           sync, then show the newest N Inbox messages (default 20)
  kweb mailboxes              sync, then list mailboxes with totals and whether they notify
  kweb show <message-id>      show a message: headers, text, attachments
  kweb search <query>         search all mail on the server (the cache when offline)
  kweb folder add <name> [parent-id]
                              make a folder (at the top level without a parent)
  kweb folder rename <id> <name>
                              rename a folder you made
  kweb folder move <id> [parent-id]
                              move a folder you made under another (to the top without one)
  kweb folder rm <id> [--with-mail]
                              delete a folder you made; with --with-mail its mail goes to
                              the Trash first (without it a folder that holds mail is refused)
  kweb folder notify <id> on|off
                              whether new mail in the folder notifies (on this device only)
  kweb spam <message-id>      report a message as spam: to Junk, marked $junk
  kweb notspam <message-id>   report it as not spam: back to the Inbox, marked $notjunk
  kweb block <message-id>     block the sender: a filter rule that deletes their mail, and
                              the message goes to the Trash
  kweb unsubscribe <message-id> [--yes]
                              leave the mailing list a message came from: says what it
                              would do, and does it with --yes (one request to the list's
                              server, or else a message to the list's address)
  kweb labels                 list the labels
  kweb label add <name>       make a label
  kweb label rm <id>          remove a label from the list (mail keeps the keyword)
  kweb label set <message-id> <label-id> on|off
                              put a label on a message or take it off
  kweb label show <label-id>  list the mail that has the label
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
  kweb files [ls] [folder-id] list the files and folders in a folder (default: the top level)
  kweb files spaces           list the places that hold files: your own and shared ones
  kweb files put <local-path> [folder-id] [--replace]
                              upload a file; --replace gives an existing file of that name
                              new content (otherwise that is an error)
  kweb files get <file-id> <local-path>
                              download a file
  kweb files mkdir <name> [folder-id]
                              make a folder
  kweb files rm <id>          delete a file, or a folder with everything in it, for good
                              every files command takes --space <account-id> to work in a
                              shared space (see files spaces) instead of your own
  kweb shared                 list the accounts of other users that share mail, calendars
                              or contacts with you, and which of the three each shares
  kweb shared mail <account> [mailbox-id]
                              list the folders an account shares, or the newest 20 messages
                              of one of them (<account> is the id that kweb shared shows)
  kweb shared show <account> <message-id>
                              show a shared message (it is not marked read)
  kweb shared agenda <account> [days]
                              show the events in the calendars an account shares (default 7)
  kweb shared contacts <account> [query]
                              list the contacts an account shares, or those matching the query
                              everything shared is read from the server each time, never kept
  kweb share list <kind> <id>
                              show whom one of your folders, calendars, address books or file
                              folders is shared with, and how (read or write); <kind> is
                              mailbox, calendar, addressbook or folder
  kweb share set <kind> <id> <person-id> read|write|none
                              share it with a person, change how, or with none take it back
  kweb share people <text>    find users to share with (a server may not allow that)
  kweb features               show what the server offers beyond mail: masked addresses,
                              the account's security settings, and whether signing in again
                              is needed to use them
  kweb masked                 list the masked addresses (newest first) and the server's limit
  kweb masked new <note> [--site <site>] [--prefix <p>]
                              have the server make one; <note> may be \"\"
  kweb masked on|off|rm <id>  switch one on or off, or delete it for good
  kweb security               show the account's security settings: whether two-step codes
                              are on, and the names of app passwords, API keys, passkeys and
                              connected apps (never a secret)
  kweb away                   show the away message
  kweb away on <text>         switch it on from now until switched off, with this text
  kweb away off               switch it off
  kweb filters                show the mail filters: whether they run, and the rules one per line
  kweb filters text           print the script the server has for them
  kweb filters on             make the filters the script the server runs (this switches off
                              the away message or another script; the app says which)
  kweb filters off            stop the filters running (nothing else is touched)
  kweb filters add-from <message-id> <folder-id>
                              add a rule: mail from that message's sender (or mailing list)
                              moves to that folder. Saved, but not switched on
  kweb send <to> <subject> <text>
                              send a plain message now (no undo delay)
  kweb send-at <minutes-from-now> <to> <subject> <text>
                              hand a plain message to the server to send in that many minutes
                              (1 to 10080: the server holds it for seven days at most)
  kweb scheduled              list the messages the server is holding back, soonest first
  kweb unschedule <id>        take a held message back: it is a draft again (id from scheduled)
  kweb identity list          list the senders with their name, reply-to and signature
  kweb identity set <id> <name> [--signature <text>] [--reply-to <address>]
                              change a sender; what is left out stays as it is, and an empty
                              text (\"\") takes the signature or the reply-to away
  kweb identity default <id>  make that sender the one new messages start with (this device)
  kweb ai status              show how the AI assistant is set up (never the key)
  kweb ai set <provider> <base-url> <model> [--effort <level>] [--yes]
                              set up that service and use it; provider is openai or anthropic,
                              each keeps its own address, model, key and consent. The key is
                              read from $KWEB_AI_KEY. Without --yes it only says what you agree
                              to. Effort (default, low, medium, high, xhigh, max) is sent to
                              Anthropic only; left out, the stored one is kept
  kweb ai models [provider]   list the models of the service (the one in use, or that provider);
                              uses $KWEB_AI_KEY if set, else the stored key
  kweb ai summary <message-id>
                              summarise a message, printed as it streams
  kweb admin [status]         show what the stored admin key opens and the server's figures
  kweb admin connect          store the admin API key in $KWEB_ADMIN_KEY (checked first)
  kweb admin forget           forget the stored admin key on this device
  kweb admin queue            the mail queue, the next to be tried first
  kweb admin accounts [text]  the server's accounts and groups, or those matching the text
  kweb admin blocked          the blocked network addresses, newest first
  kweb admin logs [text]      the newest log lines, or those with the text
  kweb admin history [hours]  the dashboard's history: 24 (default), 168 or 720 hours
  kweb admin deliveries [text]
                              the server's delivery history, newest first, with an address or queue id
  kweb admin delivery <id>    one delivery, step by step
  kweb admin deleted          deleted items the server still keeps (own account)
  kweb admin alerts           the server's alerts
  kweb admin tenants          the tenants with their disk use
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
    AdminEnterprise(crate::enterprise::Enterprise),
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
    Features,
    Masked(Masked),
    Security,
    Contacts { query: String },
    Files { space: Option<String>, what: Files },
    Shared(Shared),
    Share(Share),
    Away(Away),
    Filters(Filters),
    Folder(Folder),
    Spam { id: String, is_spam: bool },
    Block { id: String },
    Unsubscribe { id: String, yes: bool },
    Labels,
    Label(Label),
    Send { to: String, subject: String, text: String },
    SendAt { minutes: u32, to: String, subject: String, text: String },
    Scheduled,
    Unschedule { id: String },
    Identity(Identity),
    Ai(Ai),
    Admin(crate::admin::Admin),
    SignOut,
}

/// What `kweb ai` does.
#[derive(Debug, Clone, PartialEq)]
pub enum Ai {
    Status,
    Set {
        provider: AiProvider,
        base_url: String,
        model: String,
        effort: Option<AiEffort>,
        yes: bool,
    },
    /// The service in use when no provider is named.
    Models {
        provider: Option<AiProvider>,
    },
    Summary {
        id: String,
    },
}

/// What `kweb masked` does.
#[derive(Debug, Clone, PartialEq)]
pub enum Masked {
    List,
    New { note: String, site: String, prefix: String },
    Switch { id: String, on: bool },
    Rm { id: String },
}

/// What `kweb files` does.
#[derive(Debug, Clone, PartialEq)]
pub enum Files {
    List { folder: Option<String> },
    Spaces,
    Put { path: String, folder: Option<String>, replace: bool },
    Get { id: String, path: String },
    Mkdir { name: String, folder: Option<String> },
    Rm { id: String },
}

/// What `kweb shared` does.
#[derive(Debug, Clone, PartialEq)]
pub enum Shared {
    Accounts,
    Mail { account: String, mailbox: Option<String> },
    Show { account: String, id: String },
    Agenda { account: String, days: u32 },
    Contacts { account: String, query: String },
}

/// What `kweb share` does.
#[derive(Debug, Clone, PartialEq)]
pub enum Share {
    List { kind: ShareKind, id: String },
    Set { kind: ShareKind, id: String, person: String, level: Option<ShareLevel> },
    People { text: String },
}

/// What `kweb filters` does.
#[derive(Debug, Clone, PartialEq)]
pub enum Filters {
    Show,
    Text,
    On,
    Off,
    AddFrom { message_id: String, folder_id: String },
}

/// What `kweb identity` does.
#[derive(Debug, Clone, PartialEq)]
pub enum Identity {
    List,
    /// `None`: leave it as it is.
    Set {
        id: String,
        name: String,
        signature: Option<String>,
        reply_to: Option<String>,
    },
    Default {
        id: String,
    },
}

/// What `kweb folder` does.
#[derive(Debug, Clone, PartialEq)]
pub enum Folder {
    Add { name: String, parent: Option<String> },
    Rename { id: String, name: String },
    Move { id: String, parent: Option<String> },
    Rm { id: String, with_mail: bool },
    Notify { id: String, on: bool },
}

/// What `kweb label` does.
#[derive(Debug, Clone, PartialEq)]
pub enum Label {
    Add { name: String },
    Rm { id: String },
    Set { message_id: String, label_id: String, on: bool },
    Show { label_id: String },
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
        "admin" if crate::enterprise::owns(rest) => crate::enterprise::parse(rest).map(Command::AdminEnterprise),
        "status" => no_args(rest, Command::Status),
        "mailboxes" => no_args(rest, Command::Mailboxes),
        "watch" => no_args(rest, Command::Watch),
        "sign-out" => no_args(rest, Command::SignOut),
        "calendars" => no_args(rest, Command::Calendars),
        "agenda" => agenda(rest),
        "identities" => no_args(rest, Command::Identities),
        "quota" => no_args(rest, Command::Quota),
        "features" => no_args(rest, Command::Features),
        "security" => no_args(rest, Command::Security),
        "masked" => masked(rest).map(Command::Masked),
        "contacts" => Ok(Command::Contacts { query: rest.join(" ") }),
        "files" => files(rest),
        "admin" => crate::admin::parse(rest).map(Command::Admin),
        "shared" => shared(rest).map(Command::Shared),
        "share" => share(rest).map(Command::Share),
        "away" => away(rest),
        "filters" => filters(rest).map(Command::Filters),
        "folder" => folder(rest).map(Command::Folder),
        "spam" => one_arg(name, "message-id", rest).map(|id| Command::Spam { id, is_spam: true }),
        "notspam" => one_arg(name, "message-id", rest).map(|id| Command::Spam { id, is_spam: false }),
        "block" => one_arg(name, "message-id", rest).map(|id| Command::Block { id }),
        "unsubscribe" => unsubscribe(rest),
        "labels" => no_args(rest, Command::Labels),
        "label" => label(rest).map(Command::Label),
        "send" => send(rest),
        "send-at" => send_at(rest),
        "scheduled" => no_args(rest, Command::Scheduled),
        "unschedule" => one_arg(name, "id", rest).map(|id| Command::Unschedule { id }),
        "identity" => identity(rest).map(Command::Identity),
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

/// `--space <account-id>` and `--replace` may stand anywhere; the rest are the words of the
/// command. A first word that is not a command name is the folder to list.
fn files(rest: &[String]) -> Result<Command, String> {
    let (mut space, mut replace) = (None, false);
    let mut words: Vec<&str> = Vec::new();
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        if arg == "--space" {
            space = Some(args.next().ok_or("--space needs an account id")?.clone());
        } else if let Some(value) = arg.strip_prefix("--space=") {
            space = Some(value.to_string());
        } else if arg == "--replace" {
            replace = true;
        } else if arg.starts_with("--") {
            return Err(format!("unknown option '{arg}'"));
        } else {
            words.push(arg);
        }
    }
    if space.as_deref().is_some_and(str::is_empty) {
        return Err("--space needs an account id".into());
    }
    let too_many = |extra: &str| Err(format!("unexpected argument '{extra}'"));
    let what = match words.as_slice() {
        [] => Files::List { folder: None },
        // The explicit form, for a folder whose id is also a command word.
        ["ls"] => Files::List { folder: None },
        ["ls", folder] => Files::List { folder: Some(folder.to_string()) },
        ["ls", _, extra, ..] => return too_many(extra),
        ["spaces"] => Files::Spaces,
        ["spaces", extra, ..] => return too_many(extra),
        ["put"] => return Err("files put needs a <local-path>".into()),
        ["put", path] => Files::Put { path: path.to_string(), folder: None, replace },
        ["put", path, folder] => Files::Put { path: path.to_string(), folder: Some(folder.to_string()), replace },
        ["put", _, _, extra, ..] => return too_many(extra),
        ["get", id, path] => Files::Get { id: id.to_string(), path: path.to_string() },
        ["get", _, _, extra, ..] => return too_many(extra),
        ["get", ..] => return Err("files get needs a <file-id> and a <local-path>".into()),
        ["mkdir"] => return Err("files mkdir needs a <name>".into()),
        ["mkdir", name] => Files::Mkdir { name: name.to_string(), folder: None },
        ["mkdir", name, folder] => Files::Mkdir { name: name.to_string(), folder: Some(folder.to_string()) },
        ["mkdir", _, _, extra, ..] => return too_many(extra),
        ["rm", id] => Files::Rm { id: id.to_string() },
        ["rm"] => return Err("files rm needs an <id>".into()),
        ["rm", _, extra, ..] => return too_many(extra),
        [folder] => Files::List { folder: Some(folder.to_string()) },
        [_, extra, ..] => return too_many(extra),
    };
    if replace && !matches!(what, Files::Put { .. }) {
        return Err("--replace only goes with files put".into());
    }
    Ok(Command::Files { space, what })
}

fn shared(rest: &[String]) -> Result<Shared, String> {
    let too_many = |extra: &String| Err(format!("unexpected argument '{extra}'"));
    match rest {
        [] => Ok(Shared::Accounts),
        [word, tail @ ..] if word == "mail" => match tail {
            [] => Err("shared mail needs an <account>".into()),
            [account] => Ok(Shared::Mail { account: account.clone(), mailbox: None }),
            [account, mailbox] => Ok(Shared::Mail { account: account.clone(), mailbox: Some(mailbox.clone()) }),
            [_, _, extra, ..] => too_many(extra),
        },
        [word, tail @ ..] if word == "show" => match tail {
            [account, id] => Ok(Shared::Show { account: account.clone(), id: id.clone() }),
            [_, _, extra, ..] => too_many(extra),
            _ => Err("shared show needs an <account> and a <message-id>".into()),
        },
        [word, tail @ ..] if word == "agenda" => match tail {
            [] => Err("shared agenda needs an <account>".into()),
            [account] => Ok(Shared::Agenda { account: account.clone(), days: DEFAULT_AGENDA_DAYS }),
            [account, n] => match n.parse::<u32>() {
                Ok(days) if (1..=MAX_AGENDA_DAYS).contains(&days) => {
                    Ok(Shared::Agenda { account: account.clone(), days })
                }
                _ => Err(format!("shared agenda needs a number of days from 1 to {MAX_AGENDA_DAYS}, not '{n}'")),
            },
            [_, _, extra, ..] => too_many(extra),
        },
        [word, tail @ ..] if word == "contacts" => match tail {
            [] => Err("shared contacts needs an <account>".into()),
            [account, words @ ..] => Ok(Shared::Contacts { account: account.clone(), query: words.join(" ") }),
        },
        [other, ..] => Err(format!("shared takes mail, show, agenda, contacts, or nothing, not '{other}'")),
    }
}

fn share_kind(word: &str) -> Result<ShareKind, String> {
    match word {
        "mailbox" => Ok(ShareKind::Mailbox),
        "calendar" => Ok(ShareKind::Calendar),
        "addressbook" => Ok(ShareKind::AddressBook),
        "folder" => Ok(ShareKind::FileFolder),
        other => Err(format!("the kind is mailbox, calendar, addressbook or folder, not '{other}'")),
    }
}

fn share(rest: &[String]) -> Result<Share, String> {
    match rest {
        [word, tail @ ..] if word == "list" => match tail {
            [kind, id] => Ok(Share::List { kind: share_kind(kind)?, id: id.clone() }),
            [_, _, extra, ..] => Err(format!("unexpected argument '{extra}'")),
            _ => Err("share list needs a <kind> and an <id>".into()),
        },
        [word, tail @ ..] if word == "set" => match tail {
            [kind, id, person, level] => {
                let level = match level.as_str() {
                    "read" => Some(ShareLevel::Read),
                    "write" => Some(ShareLevel::Write),
                    "none" => None,
                    other => return Err(format!("the level is read, write or none, not '{other}'")),
                };
                Ok(Share::Set { kind: share_kind(kind)?, id: id.clone(), person: person.clone(), level })
            }
            [_, _, _, _, extra, ..] => Err(format!("unexpected argument '{extra}'")),
            _ => Err("share set needs a <kind>, an <id>, a <person-id> and read, write or none".into()),
        },
        [word, words @ ..] if word == "people" => match words {
            [] => Err("share people needs some text to look for".into()),
            words => Ok(Share::People { text: words.join(" ") }),
        },
        [other, ..] => Err(format!("share takes list, set or people, not '{other}'")),
        [] => Err("share takes list, set or people".into()),
    }
}

fn masked(rest: &[String]) -> Result<Masked, String> {
    let too_many = |extra: &String| Err(format!("unexpected argument '{extra}'"));
    match rest {
        [] => Ok(Masked::List),
        [word, tail @ ..] if word == "new" => {
            let (mut site, mut prefix) = (String::new(), String::new());
            let mut note: Option<String> = None;
            let mut args = tail.iter();
            while let Some(arg) = args.next() {
                let (name, inline) = match arg.split_once('=') {
                    Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
                    _ => (arg.as_str(), None),
                };
                match name {
                    "--site" | "--prefix" => {
                        let value = match inline {
                            Some(v) => v,
                            None => args.next().ok_or(format!("{name} needs a value"))?.clone(),
                        };
                        if name == "--site" {
                            site = value
                        } else {
                            prefix = value
                        }
                    }
                    other if other.starts_with("--") => return Err(format!("unknown option '{other}'")),
                    _ if note.is_none() => note = Some(arg.clone()),
                    _ => return too_many(arg),
                }
            }
            let note = note.ok_or("masked new needs a <note> (it may be \"\")")?;
            Ok(Masked::New { note, site, prefix })
        }
        [word, tail @ ..] if matches!(word.as_str(), "on" | "off" | "rm") => match tail {
            [id] if word == "rm" => Ok(Masked::Rm { id: id.clone() }),
            [id] => Ok(Masked::Switch { id: id.clone(), on: word == "on" }),
            [] => Err(format!("masked {word} needs an <id>")),
            [_, extra, ..] => too_many(extra),
        },
        [other, ..] => Err(format!("masked takes new, on, off, rm, or nothing, not '{other}'")),
    }
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

fn filters(rest: &[String]) -> Result<Filters, String> {
    let no_more = |rest: &[String]| match rest.first() {
        None => Ok(()),
        Some(extra) => Err(format!("unexpected argument '{extra}'")),
    };
    match rest {
        [] => Ok(Filters::Show),
        [word, more @ ..] if word == "text" => no_more(more).map(|()| Filters::Text),
        [word, more @ ..] if word == "on" => no_more(more).map(|()| Filters::On),
        [word, more @ ..] if word == "off" => no_more(more).map(|()| Filters::Off),
        [word, message_id, folder_id, more @ ..] if word == "add-from" => {
            no_more(more).map(|()| Filters::AddFrom { message_id: message_id.clone(), folder_id: folder_id.clone() })
        }
        [word, ..] if word == "add-from" => Err("filters add-from needs <message-id> and <folder-id>".into()),
        [other, ..] => Err(format!("filters takes text, on, off, add-from, or nothing, not '{other}'")),
    }
}

/// `--yes` may stand before or after the message id.
fn unsubscribe(rest: &[String]) -> Result<Command, String> {
    let yes = rest.iter().any(|w| w == "--yes");
    let words: Vec<String> = rest.iter().filter(|w| *w != "--yes").cloned().collect();
    one_arg("unsubscribe", "message-id", &words).map(|id| Command::Unsubscribe { id, yes })
}

fn label(rest: &[String]) -> Result<Label, String> {
    let too_many = |extra: &String| Err(format!("unexpected argument '{extra}'"));
    let Some((sub, rest)) = rest.split_first() else {
        return Err("label takes add, rm, set or show".into());
    };
    match (sub.as_str(), rest) {
        // The words are one name, so quotes are optional.
        ("add", []) => Err("label add needs a <name>".into()),
        ("add", words) => Ok(Label::Add { name: words.join(" ") }),
        ("rm", [id]) => Ok(Label::Rm { id: id.clone() }),
        ("rm", []) => Err("label rm needs an <id>".into()),
        ("rm", [_, extra, ..]) => too_many(extra),
        ("set", [message_id, label_id, state]) if state == "on" || state == "off" => {
            Ok(Label::Set { message_id: message_id.clone(), label_id: label_id.clone(), on: state == "on" })
        }
        ("set", [_, _, state]) => Err(format!("label set takes on or off, not '{state}'")),
        ("set", [_, _, _, extra, ..]) => too_many(extra),
        ("set", _) => Err("label set needs a <message-id>, a <label-id> and on or off".into()),
        ("show", [label_id]) => Ok(Label::Show { label_id: label_id.clone() }),
        ("show", []) => Err("label show needs a <label-id>".into()),
        ("show", [_, extra, ..]) => too_many(extra),
        (other, _) => Err(format!("label takes add, rm, set or show, not '{other}'")),
    }
}

fn folder(rest: &[String]) -> Result<Folder, String> {
    let too_many = |extra: &String| Err(format!("unexpected argument '{extra}'"));
    let Some((sub, rest)) = rest.split_first() else {
        return Err("folder takes add, rename, move, rm or notify".into());
    };
    match (sub.as_str(), rest) {
        ("add", []) => Err("folder add needs a <name>".into()),
        ("add", [name]) => Ok(Folder::Add { name: name.clone(), parent: None }),
        ("add", [name, parent]) => Ok(Folder::Add { name: name.clone(), parent: Some(parent.clone()) }),
        ("add", [_, _, extra, ..]) => too_many(extra),
        ("rename", [id, name]) => Ok(Folder::Rename { id: id.clone(), name: name.clone() }),
        ("rename", [_, _, extra, ..]) => too_many(extra),
        ("rename", _) => Err("folder rename needs an <id> and a <name>".into()),
        ("move", []) => Err("folder move needs an <id>".into()),
        ("move", [id]) => Ok(Folder::Move { id: id.clone(), parent: None }),
        ("move", [id, parent]) => Ok(Folder::Move { id: id.clone(), parent: Some(parent.clone()) }),
        ("move", [_, _, extra, ..]) => too_many(extra),
        ("rm", words) => {
            let with_mail = words.iter().any(|w| w == "--with-mail");
            let ids: Vec<&String> = words.iter().filter(|w| *w != "--with-mail").collect();
            match ids.as_slice() {
                [] => Err("folder rm needs an <id>".into()),
                [id] => Ok(Folder::Rm { id: (*id).clone(), with_mail }),
                [_, extra, ..] => too_many(extra),
            }
        }
        ("notify", [id, state]) if state == "on" || state == "off" => {
            Ok(Folder::Notify { id: id.clone(), on: state == "on" })
        }
        ("notify", [_, state]) => Err(format!("folder notify takes on or off, not '{state}'")),
        ("notify", [_, _, extra, ..]) => too_many(extra),
        ("notify", _) => Err("folder notify needs an <id> and on or off".into()),
        (other, _) => Err(format!("folder takes add, rename, move, rm or notify, not '{other}'")),
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

/// The most minutes ahead: seven days, the longest the server holds a message.
const MAX_MINUTES: u32 = 7 * 24 * 60;

fn send_at(rest: &[String]) -> Result<Command, String> {
    match rest {
        [minutes, to, subject, text] => {
            let minutes: u32 = minutes.trim().parse().map_err(|_| "send-at needs the minutes as a whole number")?;
            if !(1..=MAX_MINUTES).contains(&minutes) {
                return Err(format!("send-at takes 1 to {MAX_MINUTES} minutes"));
            }
            if to.trim().is_empty() {
                return Err("send-at needs a <to> address".into());
            }
            Ok(Command::SendAt { minutes, to: to.clone(), subject: subject.clone(), text: text.clone() })
        }
        [_, _, _, _, extra, ..] => Err(format!("unexpected argument '{extra}'")),
        _ => Err("send-at needs <minutes-from-now> <to> <subject> <text>".into()),
    }
}

fn identity(rest: &[String]) -> Result<Identity, String> {
    let Some((sub, rest)) = rest.split_first() else { return Err("identity needs list, set or default".into()) };
    match sub.as_str() {
        "list" => match rest.first() {
            None => Ok(Identity::List),
            Some(extra) => Err(format!("unexpected argument '{extra}'")),
        },
        "default" => one_arg("identity default", "id", rest).map(|id| Identity::Default { id }),
        "set" => {
            let [id, name, options @ ..] = rest else { return Err("identity set needs an <id> and a <name>".into()) };
            let (mut signature, mut reply_to) = (None, None);
            let mut options = options.iter();
            while let Some(option) = options.next() {
                let slot = match option.as_str() {
                    "--signature" => &mut signature,
                    "--reply-to" => &mut reply_to,
                    other => return Err(format!("unexpected argument '{other}'")),
                };
                let value = options.next().ok_or_else(|| format!("{option} needs a value"))?;
                if slot.replace(value.clone()).is_some() {
                    return Err(format!("{option} was given twice"));
                }
            }
            Ok(Identity::Set { id: id.clone(), name: name.clone(), signature, reply_to })
        }
        other => Err(format!("identity takes list, set or default, not '{other}'")),
    }
}

fn ai(rest: &[String]) -> Result<Ai, String> {
    match rest.split_first() {
        None => Err("ai needs status, set, models or summary".into()),
        Some((sub, rest)) => match sub.as_str() {
            "status" => no_args(rest, Command::Status).map(|_| Ai::Status),
            "summary" => one_arg("ai summary", "message-id", rest).map(|id| Ai::Summary { id }),
            "set" => ai_set(rest),
            "models" => match rest {
                [] => Ok(Ai::Models { provider: None }),
                [one] => provider_named(one).map(|p| Ai::Models { provider: Some(p) }),
                [_, extra, ..] => Err(format!("unexpected argument '{extra}'")),
            },
            other => Err(format!("ai takes status, set, models or summary, not '{other}'")),
        },
    }
}

fn provider_named(name: &str) -> Result<AiProvider, String> {
    match name.to_ascii_lowercase().as_str() {
        "openai" | "openai-compatible" => Ok(AiProvider::OpenAiCompatible),
        "anthropic" => Ok(AiProvider::Anthropic),
        other => Err(format!("the provider is openai or anthropic, not '{other}'")),
    }
}

fn effort_named(name: &str) -> Result<AiEffort, String> {
    match name.to_ascii_lowercase().as_str() {
        "default" => Ok(AiEffort::Default),
        "low" => Ok(AiEffort::Low),
        "medium" => Ok(AiEffort::Medium),
        "high" => Ok(AiEffort::High),
        "xhigh" => Ok(AiEffort::XHigh),
        "max" => Ok(AiEffort::Max),
        other => Err(format!("the effort is default, low, medium, high, xhigh or max, not '{other}'")),
    }
}

/// The key is never an argument: it is read from the environment (see `kweb help`).
fn ai_set(rest: &[String]) -> Result<Ai, String> {
    let mut yes = false;
    let mut effort = None;
    let mut words: Vec<&String> = Vec::new();
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        if arg == "--yes" {
            yes = true;
        } else if arg == "--effort" {
            let value = args.next().ok_or("--effort needs a level")?;
            effort = Some(effort_named(value)?);
        } else if let Some(value) = arg.strip_prefix("--effort=") {
            effort = Some(effort_named(value)?);
        } else if arg.starts_with("--") {
            return Err(format!("unknown option '{arg}'"));
        } else {
            words.push(arg);
        }
    }
    let [provider, base_url, model] = words[..] else {
        return Err("ai set needs <provider> <base-url> <model>".into());
    };
    Ok(Ai::Set { provider: provider_named(provider)?, base_url: base_url.clone(), model: model.clone(), effort, yes })
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
    fn unsubscribe_and_label_commands() {
        assert_eq!(p(&["unsubscribe", "e1"]), Ok(Command::Unsubscribe { id: "e1".into(), yes: false }));
        assert_eq!(p(&["unsubscribe", "e1", "--yes"]), Ok(Command::Unsubscribe { id: "e1".into(), yes: true }));
        assert_eq!(p(&["unsubscribe", "--yes", "e1"]), p(&["unsubscribe", "e1", "--yes"]));
        assert!(p(&["unsubscribe"]).is_err() && p(&["unsubscribe", "a", "b"]).is_err());
        assert_eq!(p(&["labels"]), Ok(Command::Labels));
        assert!(p(&["labels", "x"]).is_err());
        let l = |w: &[&str]| {
            let mut args = vec!["label"];
            args.extend_from_slice(w);
            p(&args)
        };
        assert_eq!(l(&["add", "Work"]), Ok(Command::Label(Label::Add { name: "Work".into() })));
        assert_eq!(l(&["add", "Mom", "and", "Dad"]), Ok(Command::Label(Label::Add { name: "Mom and Dad".into() })));
        assert_eq!(l(&["rm", "work"]), Ok(Command::Label(Label::Rm { id: "work".into() })));
        assert_eq!(
            l(&["set", "e1", "work", "on"]),
            Ok(Command::Label(Label::Set { message_id: "e1".into(), label_id: "work".into(), on: true }))
        );
        assert_eq!(
            l(&["set", "e1", "work", "off"]),
            Ok(Command::Label(Label::Set { message_id: "e1".into(), label_id: "work".into(), on: false }))
        );
        assert_eq!(l(&["show", "work"]), Ok(Command::Label(Label::Show { label_id: "work".into() })));
        assert!(l(&["set", "e1", "work", "maybe"]).is_err() && l(&["set", "e1", "work"]).is_err());
        assert!(l(&[]).is_err() && l(&["add"]).is_err() && l(&["rm"]).is_err() && l(&["show", "a", "b"]).is_err());
        assert!(l(&["bogus"]).is_err() && l(&["rm", "a", "b"]).is_err());
    }

    #[test]
    fn folder_commands() {
        let f = |w: &[&str]| {
            let mut args = vec!["folder"];
            args.extend_from_slice(w);
            p(&args)
        };
        assert_eq!(f(&["add", "Work"]), Ok(Command::Folder(Folder::Add { name: "Work".into(), parent: None })));
        assert_eq!(
            f(&["add", "Bills", "mb7"]),
            Ok(Command::Folder(Folder::Add { name: "Bills".into(), parent: Some("mb7".into()) }))
        );
        assert_eq!(
            f(&["rename", "mb7", "Jobs"]),
            Ok(Command::Folder(Folder::Rename { id: "mb7".into(), name: "Jobs".into() }))
        );
        assert_eq!(f(&["move", "mb7"]), Ok(Command::Folder(Folder::Move { id: "mb7".into(), parent: None })));
        assert_eq!(
            f(&["move", "mb7", "mb8"]),
            Ok(Command::Folder(Folder::Move { id: "mb7".into(), parent: Some("mb8".into()) }))
        );
        assert_eq!(f(&["rm", "mb7"]), Ok(Command::Folder(Folder::Rm { id: "mb7".into(), with_mail: false })));
        assert_eq!(
            f(&["rm", "--with-mail", "mb7"]),
            Ok(Command::Folder(Folder::Rm { id: "mb7".into(), with_mail: true }))
        );
        assert_eq!(f(&["rm", "mb7", "--with-mail"]), f(&["rm", "--with-mail", "mb7"]));
        assert_eq!(f(&["notify", "mb7", "on"]), Ok(Command::Folder(Folder::Notify { id: "mb7".into(), on: true })));
        assert_eq!(f(&["notify", "mb7", "off"]), Ok(Command::Folder(Folder::Notify { id: "mb7".into(), on: false })));
        assert!(f(&["notify", "mb7", "maybe"]).is_err());
        assert!(f(&["notify", "mb7"]).is_err());
        assert!(f(&["add"]).is_err() && f(&["rm"]).is_err() && f(&["rename", "a"]).is_err() && f(&[]).is_err());
        assert!(f(&["add", "a", "b", "c"]).is_err() && f(&["bogus"]).is_err());
        assert_eq!(p(&["spam", "e1"]), Ok(Command::Spam { id: "e1".into(), is_spam: true }));
        assert_eq!(p(&["notspam", "e1"]), Ok(Command::Spam { id: "e1".into(), is_spam: false }));
        assert_eq!(p(&["block", "e1"]), Ok(Command::Block { id: "e1".into() }));
        assert!(p(&["block"]).is_err() && p(&["spam", "a", "b"]).is_err());
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
            Ok(Command::Ai(Ai::Set {
                provider,
                base_url: "https://a.example/v1".into(),
                model: "m".into(),
                effort: None,
                yes,
            }))
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
        let with_effort = |args: &[&str]| match p(args) {
            Ok(Command::Ai(Ai::Set { effort, yes, .. })) => (effort, yes),
            other => panic!("{other:?}"),
        };
        let base = ["ai", "set", "anthropic", "https://a.example/v1", "m"];
        fn plus<'a>(base: &[&'a str], more: &[&'a str]) -> Vec<&'a str> {
            [base, more].concat()
        }
        assert_eq!(with_effort(&plus(&base, &["--effort", "XHigh"])), (Some(AiEffort::XHigh), false));
        assert_eq!(with_effort(&plus(&base, &["--effort=low", "--yes"])), (Some(AiEffort::Low), true));
        assert_eq!(with_effort(&plus(&base, &["--yes", "--effort", "default"])), (Some(AiEffort::Default), true));
        assert_eq!(p(&["ai", "models"]), Ok(Command::Ai(Ai::Models { provider: None })));
        assert_eq!(
            p(&["ai", "models", "Anthropic"]),
            Ok(Command::Ai(Ai::Models { provider: Some(AiProvider::Anthropic) }))
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
            &["ai", "set", "openai", "https://a.example/v1", "m", "--effort"],
            &["ai", "set", "openai", "https://a.example/v1", "m", "--effort", "huge"],
            &["ai", "models", "gemini"],
            &["ai", "models", "openai", "x"],
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
    fn sending_later_and_the_identity_commands() {
        assert_eq!(
            p(&["send-at", "3", "bo@example.test", "Hi", "Hello"]),
            Ok(Command::SendAt {
                minutes: 3,
                to: "bo@example.test".into(),
                subject: "Hi".into(),
                text: "Hello".into()
            })
        );
        for bad in [
            &["send-at", "0", "a@b", "s", "t"][..],
            &["send-at", "10081", "a@b", "s", "t"],
            &["send-at", "x", "a@b", "s", "t"],
            &["send-at", "3", " ", "s", "t"],
            &["send-at", "3", "a@b", "s"],
            &["send-at", "3", "a@b", "s", "t", "u"],
        ] {
            assert!(p(bad).is_err(), "{bad:?}");
        }
        assert_eq!(p(&["scheduled"]), Ok(Command::Scheduled));
        assert!(p(&["scheduled", "x"]).is_err());
        assert_eq!(p(&["unschedule", "s1"]), Ok(Command::Unschedule { id: "s1".into() }));
        assert!(p(&["unschedule"]).is_err());
        assert_eq!(p(&["identity", "list"]), Ok(Command::Identity(Identity::List)));
        assert_eq!(p(&["identity", "default", "i2"]), Ok(Command::Identity(Identity::Default { id: "i2".into() })));
        assert_eq!(
            p(&["identity", "set", "i1", "Me", "--signature", "", "--reply-to", "r@x.org"]),
            Ok(Command::Identity(Identity::Set {
                id: "i1".into(),
                name: "Me".into(),
                signature: Some(String::new()),
                reply_to: Some("r@x.org".into())
            }))
        );
        assert_eq!(
            p(&["identity", "set", "i1", "Me"]),
            Ok(Command::Identity(Identity::Set {
                id: "i1".into(),
                name: "Me".into(),
                signature: None,
                reply_to: None
            }))
        );
        for bad in [
            &["identity"][..],
            &["identity", "set", "i1"],
            &["identity", "set", "i1", "Me", "--signature"],
            &["identity", "set", "i1", "Me", "--signature", "a", "--signature", "b"],
            &["identity", "set", "i1", "Me", "--other", "a"],
            &["identity", "nope"],
        ] {
            assert!(p(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn features_masked_and_security() {
        assert_eq!(p(&["features"]), Ok(Command::Features));
        assert_eq!(p(&["security"]), Ok(Command::Security));
        assert!(p(&["security", "x"]).is_err());
        assert_eq!(p(&["masked"]), Ok(Command::Masked(Masked::List)));
        assert_eq!(
            p(&["masked", "new", "shop", "--site", "https://s.example", "--prefix", "shop"]),
            Ok(Command::Masked(Masked::New {
                note: "shop".into(),
                site: "https://s.example".into(),
                prefix: "shop".into()
            }))
        );
        assert_eq!(
            p(&["masked", "new", "--prefix=a", "", "--site=x"]),
            Ok(Command::Masked(Masked::New { note: "".into(), site: "x".into(), prefix: "a".into() }))
        );
        assert!(p(&["masked", "new"]).is_err());
        assert!(p(&["masked", "new", "a", "b"]).is_err());
        assert!(p(&["masked", "new", "a", "--site"]).is_err());
        assert!(p(&["masked", "new", "a", "--colour", "red"]).is_err());
        assert_eq!(p(&["masked", "on", "m1"]), Ok(Command::Masked(Masked::Switch { id: "m1".into(), on: true })));
        assert_eq!(p(&["masked", "off", "m1"]), Ok(Command::Masked(Masked::Switch { id: "m1".into(), on: false })));
        assert_eq!(p(&["masked", "rm", "m1"]), Ok(Command::Masked(Masked::Rm { id: "m1".into() })));
        assert!(p(&["masked", "rm"]).is_err());
        assert!(p(&["masked", "on", "a", "b"]).is_err());
        assert!(p(&["masked", "frob"]).is_err());
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
    fn share_commands() {
        let s = |what| Ok(Command::Share(what));
        assert_eq!(
            p(&["share", "list", "calendar", "c1"]),
            s(Share::List { kind: ShareKind::Calendar, id: "c1".into() })
        );
        assert_eq!(
            p(&["share", "set", "mailbox", "m", "d", "write"]),
            s(Share::Set {
                kind: ShareKind::Mailbox,
                id: "m".into(),
                person: "d".into(),
                level: Some(ShareLevel::Write)
            })
        );
        assert_eq!(
            p(&["share", "set", "folder", "f", "d", "none"]),
            s(Share::Set { kind: ShareKind::FileFolder, id: "f".into(), person: "d".into(), level: None })
        );
        assert_eq!(p(&["share", "people", "ann", "lee"]), s(Share::People { text: "ann lee".into() }));
        assert!(p(&["share", "set", "mailbox", "m", "d", "all"]).is_err());
        assert!(p(&["share", "list", "contacts", "x"]).is_err());
        assert!(p(&["share", "list", "mailbox"]).is_err());
        assert!(p(&["share", "people"]).is_err());
        assert!(p(&["share"]).is_err());
    }

    #[test]
    fn shared_commands() {
        let s = |what| Ok(Command::Shared(what));
        let a = || "sh1".to_string();
        assert_eq!(p(&["shared"]), s(Shared::Accounts));
        assert_eq!(p(&["shared", "mail", "sh1"]), s(Shared::Mail { account: a(), mailbox: None }));
        assert_eq!(p(&["shared", "mail", "sh1", "m9"]), s(Shared::Mail { account: a(), mailbox: Some("m9".into()) }));
        assert!(p(&["shared", "mail"]).is_err());
        assert!(p(&["shared", "mail", "a", "b", "c"]).is_err());
        assert_eq!(p(&["shared", "show", "sh1", "e1"]), s(Shared::Show { account: a(), id: "e1".into() }));
        assert!(p(&["shared", "show", "sh1"]).is_err());
        assert_eq!(p(&["shared", "agenda", "sh1"]), s(Shared::Agenda { account: a(), days: 7 }));
        assert_eq!(p(&["shared", "agenda", "sh1", "30"]), s(Shared::Agenda { account: a(), days: 30 }));
        assert!(p(&["shared", "agenda", "sh1", "0"]).is_err());
        assert!(p(&["shared", "agenda"]).is_err());
        assert_eq!(p(&["shared", "contacts", "sh1"]), s(Shared::Contacts { account: a(), query: String::new() }));
        assert_eq!(
            p(&["shared", "contacts", "sh1", "ann", "lee"]),
            s(Shared::Contacts { account: a(), query: "ann lee".into() })
        );
        assert!(p(&["shared", "contacts"]).is_err());
        assert!(p(&["shared", "files"]).is_err());
    }

    #[test]
    fn files_commands() {
        let f = |space: Option<&str>, what| Ok(Command::Files { space: space.map(str::to_string), what });
        assert_eq!(p(&["files"]), f(None, Files::List { folder: None }));
        assert_eq!(p(&["files", "d1"]), f(None, Files::List { folder: Some("d1".into()) }));
        assert_eq!(p(&["files", "spaces"]), f(None, Files::Spaces));
        // A folder whose id is a command word is listed with `ls`.
        assert_eq!(p(&["files", "ls", "rm"]), f(None, Files::List { folder: Some("rm".into()) }));
        assert_eq!(p(&["files", "ls", "get"]), f(None, Files::List { folder: Some("get".into()) }));
        assert_eq!(p(&["files", "ls"]), f(None, Files::List { folder: None }));
        assert!(p(&["files", "ls", "a", "b"]).is_err());
        assert_eq!(p(&["files", "--space", "x", "d1"]), f(Some("x"), Files::List { folder: Some("d1".into()) }));
        assert_eq!(
            p(&["files", "put", "a.txt", "d1", "--replace", "--space=x"]),
            f(Some("x"), Files::Put { path: "a.txt".into(), folder: Some("d1".into()), replace: true })
        );
        assert_eq!(p(&["files", "get", "f1", "out"]), f(None, Files::Get { id: "f1".into(), path: "out".into() }));
        assert_eq!(p(&["files", "mkdir", "N"]), f(None, Files::Mkdir { name: "N".into(), folder: None }));
        assert_eq!(p(&["files", "rm", "f1"]), f(None, Files::Rm { id: "f1".into() }));
        assert!(p(&["files", "put"]).is_err() && p(&["files", "rm"]).is_err() && p(&["files", "get", "f1"]).is_err());
        assert!(p(&["files", "--space"]).is_err() && p(&["files", "--bogus"]).is_err());
        assert!(p(&["files", "rm", "f1", "--replace"]).is_err());
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
    fn filters_shows_prints_switches_and_adds() {
        let f = |w: Filters| Ok(Command::Filters(w));
        assert_eq!(p(&["filters"]), f(Filters::Show));
        assert_eq!(p(&["filters", "text"]), f(Filters::Text));
        assert_eq!(p(&["filters", "on"]), f(Filters::On));
        assert_eq!(p(&["filters", "off"]), f(Filters::Off));
        assert_eq!(
            p(&["filters", "add-from", "m1", "mb2"]),
            f(Filters::AddFrom { message_id: "m1".into(), folder_id: "mb2".into() })
        );
        assert!(p(&["filters", "add-from", "m1"]).is_err());
        assert!(p(&["filters", "on", "now"]).unwrap_err().contains("now"));
        assert!(p(&["filters", "maybe"]).unwrap_err().contains("maybe"));
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
