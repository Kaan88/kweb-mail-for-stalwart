//! `kweb`: a developer tool that drives kweb-core against a real Stalwart server from the
//! desktop. It is synchronous on purpose: every `Core` call blocks and owns its own runtime.

mod ai;
mod args;
mod commands;
mod secrets;
mod timefmt;

use std::{
    fs::{File, TryLockError},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

use args::Command;
use kweb_core::ffi::Core;
use secrets::FileSecrets;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let command = match args::parse(&argv) {
        Ok(command) => command,
        Err(problem) => {
            eprintln!("kweb: {problem}\n\n{}", args::USAGE);
            return ExitCode::from(2);
        }
    };
    if command == Command::Help {
        println!("{}", args::USAGE);
        return ExitCode::SUCCESS;
    }
    match run(command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("kweb: {message}");
            ExitCode::from(1)
        }
    }
}

fn run(command: Command) -> Result<(), String> {
    let dir = args::data_dir(|name| std::env::var(name).ok(), cfg!(windows))
        .ok_or("cannot tell where to keep data: set KWEB_HOME")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    // Kept until the process ends.
    let _only_watcher = if command == Command::Watch { Some(watch_lock(&dir)?) } else { None };
    commands::run(&open_core(&dir)?, command)
}

/// Two watchers on one cache take each mail from each other: whichever runs the notification
/// rule first prints the line, and the other stays silent. So only one may run.
fn watch_lock(dir: &Path) -> Result<File, String> {
    let path = dir.join("watch.lock");
    let file = File::create(&path).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err("another kweb watch is already running; stop it first".into()),
        Err(TryLockError::Error(e)) => Err(format!("cannot lock {}: {e}", path.display())),
    }
}

fn open_core(dir: &Path) -> Result<Arc<Core>, String> {
    let db: PathBuf = dir.join("cache.db");
    let secrets = FileSecrets::new(dir.join("secrets.json"));
    Core::new(db.to_string_lossy().into_owned(), Arc::new(secrets)).map_err(|e| e.to_string())
}
