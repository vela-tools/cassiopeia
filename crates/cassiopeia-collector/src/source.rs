use crate::file_extension::FileExtension;
use cassiopeia_common::input::Input;

/// The source configuration for the Collector stage.
#[derive(Debug, Clone)]
pub enum CollectorSource {
    /// A single input source.
    Single {
        /// The input to collect.
        input: Input,
        /// The file extension used to name the downloaded file.
        extension: FileExtension,
    },
    /// Multiple input sources.
    Multiple {
        /// The inputs to collect.
        inputs: Vec<Input>,
        /// The file extension used to name each downloaded file.
        extension: FileExtension,
    },
}
