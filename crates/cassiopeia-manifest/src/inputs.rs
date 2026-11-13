use crate::{
    error::{ManifestError, Result},
    input::ManifestInput,
};
use cassiopeia_sensorthings::{cursor_name::CursorName, service::SensorThingsService, settings::read_mode::ReadMode};
use derive_more::Deref;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

/// The sources a run reads, guaranteed to hold at least one, and no two through one cursor.
///
/// A manifest with no inputs describes no work, and two incremental inputs that share a cursor would
/// each move the positions the other resumes from, so both are rejected at construction rather than
/// discovered by a later validation pass. Deserialization routes through
/// [`TryFrom<Vec<ManifestInput>>`], so these rules run once the sequence has been read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Deref)]
#[serde(try_from = "Vec<ManifestInput>")]
pub struct Inputs(Vec<ManifestInput>);

impl Inputs {
    /// Builds the input list, rejecting an empty one and one where two inputs name one cursor.
    ///
    /// # Errors
    /// Returns [`ManifestError::NoInputs`] when `inputs` is empty, and
    /// [`ManifestError::DuplicateCursor`] naming the first cursor a second input names.
    pub fn new(inputs: Vec<ManifestInput>) -> Result<Inputs> {
        if inputs.is_empty() {
            return Err(ManifestError::NoInputs);
        }
        let mut cursors: BTreeSet<&CursorName> = BTreeSet::new();
        if let Some(repeated) = inputs.iter().filter_map(cursor).find(|name| !cursors.insert(name)) {
            // The error names the cursor, while the input keeps its own.
            return Err(ManifestError::DuplicateCursor(repeated.clone()));
        }
        Ok(Inputs(inputs))
    }

    /// Resolves every input's relative local paths against `directory`, as
    /// [`ManifestInput::resolved_against`] describes.
    #[must_use]
    pub fn resolved_against(self, directory: &Path) -> Inputs {
        Inputs(self.0.into_iter().map(|input| input.resolved_against(directory)).collect())
    }
}

/// The cursor `input` reads incrementally through, if it reads a SensorThings server that way.
fn cursor(input: &ManifestInput) -> Option<&CursorName> {
    match input.sensorthings().as_ref().map(SensorThingsService::settings)?.read() {
        ReadMode::Incremental(read) => Some(read.cursor()),
        ReadMode::Latest | ReadMode::Range(_) => None,
    }
}

impl TryFrom<Vec<ManifestInput>> for Inputs {
    type Error = ManifestError;

    fn try_from(inputs: Vec<ManifestInput>) -> Result<Inputs> {
        Inputs::new(inputs)
    }
}

#[cfg(test)]
mod tests {
    use crate::{input::ManifestInput, inputs::Inputs, mapping_binding::MappingBinding};
    use cassiopeia_common::input::Input;
    use pretty_assertions::assert_eq;
    use std::path::{Path, PathBuf};

    fn an_input() -> ManifestInput {
        ManifestInput::builder()
            .source(Input::Local(PathBuf::from("data/stations.csv")))
            .mapping_binding(MappingBinding::Single {
                mapping: PathBuf::from("mappings/station.json5"),
            })
            .build()
    }

    #[test]
    fn an_empty_list_is_rejected() {
        assert!(Inputs::new(Vec::new()).is_err());
        assert!(serde_json::from_str::<Inputs>("[]").is_err());
    }

    /// An input that reads `root` incrementally through the cursor `cursor`.
    fn incremental(root: &str, cursor: &str) -> String {
        format!(
            r#"{{"source": "{root}", "format": "sensorthings", "mapping": "m.json5", "sensorthings": {{"read": {{"incremental": {{"cursor": "{cursor}"}}}}}}}}"#
        )
    }

    #[test]
    fn two_inputs_with_one_cursor_name_are_refused() {
        let json = format!(
            "[{}, {}]",
            incremental("https://a.example/v1.1", "rivm-air"),
            incremental("https://b.example/v1.1", "rivm-air")
        );

        let error = serde_json::from_str::<Inputs>(&json).unwrap_err();

        assert!(error.to_string().contains("The cursor 'rivm-air' is named by more than one input"), "{error}");
    }

    #[test]
    fn inputs_with_distinct_cursors_are_kept() {
        let json = format!(
            "[{}, {}, {}]",
            incremental("https://a.example/v1.1", "rivm-air"),
            incremental("https://a.example/v1.1", "rivm-noise"),
            r#"{"source": "https://a.example/v1.1", "format": "sensorthings", "mapping": "m.json5"}"#
        );

        assert_eq!(serde_json::from_str::<Inputs>(&json).unwrap().len(), 3);
    }

    #[test]
    fn a_populated_list_derefs_to_its_inputs() {
        let inputs = Inputs::new(vec![an_input()]).unwrap();

        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs.first(), Some(&an_input()));
    }

    #[test]
    fn resolving_a_list_resolves_every_input_against_the_given_directory() {
        let inputs = Inputs::new(vec![an_input(), an_input()]).unwrap().resolved_against(Path::new("runs/city"));

        assert_eq!(inputs.len(), 2);
        assert!(
            inputs
                .iter()
                .all(|input| input.source() == &Input::Local(PathBuf::from("runs/city/data/stations.csv")))
        );
    }
}
