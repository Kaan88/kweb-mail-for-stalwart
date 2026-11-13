//! `kweb`: a developer tool that drives kweb-core against a real Stalwart server from the
//! desktop. It is synchronous on purpose: every `Core` call blocks and owns its own runtime.

mod args;
mod commands;
mod secrets;
mod timefmt;

use std::{path::PathBuf, process::ExitCode, sync::Arc};

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
    match open_core().and_then(|core| commands::run(&core, command)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("kweb: {message}");
            ExitCode::from(1)
        }
    }
}

fn open_core() -> Result<Arc<Core>, String> {
    let dir = args::data_dir(|name| std::env::var(name).ok(), cfg!(windows))
        .ok_or("cannot tell where to keep data: set KWEB_HOME")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let db: PathBuf = dir.join("cache.db");
    let secrets = FileSecrets::new(dir.join("secrets.json"));
    Core::new(db.to_string_lossy().into_owned(), Arc::new(secrets)).map_err(|e| e.to_string())
}
