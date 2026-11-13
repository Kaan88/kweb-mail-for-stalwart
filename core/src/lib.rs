//! kweb-core: everything that talks to the Stalwart server or decides what the phone shows.
//! The Kotlin side calls [`ffi`] and renders what comes back; it never talks to the server.

pub mod actions;
#[cfg(target_os = "android")]
mod android;
pub mod auth;
pub mod error;
pub mod ffi;
pub mod jmap;
pub mod model;
pub mod notify;
pub mod push;
pub mod secrets;
pub mod store;
pub mod sync;

pub use error::{Error, Result};

uniffi::setup_scaffolding!();
