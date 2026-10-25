pub mod applicator;
pub mod duckwm;
pub mod parser;
pub mod watchers;

pub use applicator::LedApplicator;
pub use parser::parse_config;
pub use parser::parse_mmsg_output;
pub use parser::parse_quack_output;
pub use watchers::{ColorSpec, Condition, Config, ConfigWatcher, KeyPosition, Mode, Rule, Value};
