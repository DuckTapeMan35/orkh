pub mod applicator;
pub mod duckwm;
pub mod parser;
pub mod watchers;

pub use applicator::LedApplicator;
pub use parser::parse_config;
pub use watchers::{ColorSpec, Condition, Config, ConfigWatcher, KeyPosition, Mode, Rule, Value};
