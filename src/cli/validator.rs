use cassiopeia_common::{representation::NgsiLdRepresentation, schema_source::SchemaSource, skip_null::NgsiLdSkipNull};
use cassiopeia_manifest::output::validation_mode::ValidationMode;
use clap::Args;
use std::{path::PathBuf, str::FromStr};

/// Arguments controlling how a run validates entities before writing them.
#[derive(Args, Debug)]
pub struct ValidatorArgs {
    /// NGSI-LD representation the validator serializes entities in.
    #[arg(long = "validation-representation", value_enum, help_heading = "Validation", value_name = "REPRESENTATION")]
    pub representation: Option<NgsiLdRepresentation>,

    /// How the validator handles null values.
    #[arg(long = "validation-skip-null", value_enum, help_heading = "Validation", value_name = "SKIPNULL")]
    pub skip_null: Option<NgsiLdSkipNull>,

    /// How strictly schema validation is enforced, overriding the manifest: warn, fail-when-schema,
    /// or fail.
    #[arg(long = "validation-mode", help_heading = "Validation", value_name = "MODE")]
    pub validation_mode: Option<ValidationMode>,

    /// A custom JSON Schema to validate against (a local file path or an `http(s)` URL), applied to
    /// every produced type in place of the Smart Data Models convention. A per-input `schema` in the
    /// manifest wins over this.
    #[arg(long = "validation-schema", help_heading = "Validation", value_name = "FILE|URL", value_parser = SchemaSource::from_str)]
    pub schema: Option<SchemaSource>,

    /// Write a JSON validation report to the given file path.
    #[arg(short = 'r', long = "validation-report", help_heading = "Validation", value_name = "FILE")]
    pub report: Option<PathBuf>,
}
