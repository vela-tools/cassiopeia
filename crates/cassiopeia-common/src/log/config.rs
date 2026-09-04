use crate::log::{format::LogFormat, level::LogLevel};
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;
use std::path::PathBuf;

/// Configuration for the console (terminal) log sink.
///
/// Data only: how the sink behaves once its records reach a subscriber is the concern of whatever
/// crate builds that subscriber, not of this schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SmartDefault)]
#[serde(default)]
pub struct ConsoleLogConfig {
    /// Whether the console sink emits anything at all.
    #[default = true]
    pub enabled: bool,

    /// The lowest level that reaches the console.
    #[default(LogLevel::Info)]
    pub level: LogLevel,

    /// How each console record is laid out.
    #[default(LogFormat::Compact)]
    pub format: LogFormat,
}

/// Configuration for the file log sink.
///
/// Data only: the directory is stored verbatim. A deployment-specific override (such as a container's
/// conventional log location) is applied by the subscriber-building crate, not encoded here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SmartDefault)]
#[serde(default)]
pub struct FileLogConfig {
    /// Whether the file sink emits anything at all.
    #[default = true]
    pub enabled: bool,

    /// The lowest level that reaches the file.
    #[default(LogLevel::Info)]
    pub level: LogLevel,

    /// How each file record is laid out. Files default to JSON so they can be shipped to a log store.
    #[default(LogFormat::Json)]
    pub format: LogFormat,

    /// The directory the log file is written into.
    #[default(PathBuf::from("logs"))]
    pub directory: PathBuf,

    /// The name of the log file inside `directory`.
    #[default(PathBuf::from("cassiopeia.log"))]
    pub file_name: PathBuf,
}

/// The complete logging configuration: one console sink and one file sink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LoggerConfig {
    /// The terminal sink.
    pub console: ConsoleLogConfig,

    /// The file sink.
    pub file: FileLogConfig,
}

#[cfg(test)]
mod tests {
    use crate::log::{
        config::{ConsoleLogConfig, FileLogConfig, LoggerConfig},
        format::LogFormat,
        level::LogLevel,
    };
    use std::path::PathBuf;

    #[test]
    fn the_console_sink_defaults_to_enabled_compact_records_at_info_level() {
        let console = ConsoleLogConfig::default();

        assert!(console.enabled);
        assert_eq!(console.level, LogLevel::Info);
        assert_eq!(console.format, LogFormat::Compact);
    }

    #[test]
    fn the_file_sink_defaults_to_enabled_json_records_in_the_logs_directory() {
        let file = FileLogConfig::default();

        assert!(file.enabled);
        assert_eq!(file.level, LogLevel::Info);
        assert_eq!(file.format, LogFormat::Json);
        assert_eq!(file.directory, PathBuf::from("logs"));
        assert_eq!(file.file_name, PathBuf::from("cassiopeia.log"));
    }

    #[test]
    fn an_omitted_section_falls_back_to_its_own_defaults() {
        let logger: LoggerConfig = serde_json::from_str(r#"{"console":{"enabled":false}}"#).unwrap();

        assert!(!logger.console.enabled);
        assert_eq!(logger.console.level, LogLevel::Info);
        assert_eq!(logger.file, FileLogConfig::default());
    }

    #[test]
    fn a_fully_stated_file_section_round_trips() {
        let file: FileLogConfig =
            serde_json::from_str(r#"{"enabled":true,"level":"warn","format":"compact","directory":"/var/log/run","file_name":"run.log"}"#).unwrap();

        assert_eq!(file.level, LogLevel::Warn);
        assert_eq!(file.directory, PathBuf::from("/var/log/run"));
        assert_eq!(file.file_name, PathBuf::from("run.log"));
    }
}
