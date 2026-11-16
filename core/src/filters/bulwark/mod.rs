//! Filters that share Bulwark's script (M12 stage 2): its rule model, its generator, and the
//! way it reads a script back. See `dev/notes/bulwark-filters.md`.

pub mod check;
pub mod forward;
pub mod generate;
pub mod json;
pub mod migrate;
pub mod model;
pub mod period;
pub mod read;
pub mod store;
pub use generate::{generate, Options};
pub use model::{Audience, Forward, Metadata, Rule};
pub use read::{read, Parsed, Reading};
