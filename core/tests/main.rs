//! All the integration tests of the core, in one test target (one binary to link, not one per
//! file). Each file under `suite/` is a module of this crate and keeps its own name, so a single
//! test is run as `cargo test -p kweb-core --test main <file>::<test>`.

mod support;

#[path = "suite/actions.rs"]
mod actions;
#[path = "suite/admin.rs"]
mod admin;
#[path = "suite/assistant.rs"]
mod assistant;
#[path = "suite/assistant_chat.rs"]
mod assistant_chat;
#[path = "suite/assistant_converse.rs"]
mod assistant_converse;
#[path = "suite/calendar.rs"]
mod calendar;
#[path = "suite/contacts.rs"]
mod contacts;
#[path = "suite/crypto_raw.rs"]
mod crypto_raw;
#[path = "suite/enterprise.rs"]
mod enterprise;
#[path = "suite/ffi.rs"]
mod ffi;
#[path = "suite/files.rs"]
mod files;
#[path = "suite/filters.rs"]
mod filters;
#[path = "suite/folders.rs"]
mod folders;
#[path = "suite/labels.rs"]
mod labels;
#[path = "suite/manage.rs"]
mod manage;
#[path = "suite/mock_server.rs"]
mod mock_server;
#[path = "suite/push.rs"]
mod push;
#[path = "suite/reading.rs"]
mod reading;
#[path = "suite/send_later.rs"]
mod send_later;
#[path = "suite/sending.rs"]
mod sending;
#[path = "suite/shared.rs"]
mod shared;
#[path = "suite/sharing.rs"]
mod sharing;
#[path = "suite/sync.rs"]
mod sync;
#[path = "suite/unsubscribe.rs"]
mod unsubscribe;
#[path = "suite/webpush.rs"]
mod webpush;
