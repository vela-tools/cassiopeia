use crate::{
    error::{ManifestError, Result},
    input::ManifestInput,
};
use derive_more::Deref;
use serde::{Deserialize, Serialize};

/// The sources a run reads, guaranteed to hold at least one.
///
/// A manifest with no inputs describes no work, so emptiness is rejected at construction rather
/// than discovered by a later validation pass. Deserialization routes through
/// [`TryFrom<Vec<ManifestInput>>`], so the emptiness rule runs once the sequence has been read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Deref)]
#[serde(try_from = "Vec<ManifestInput>")]
pub struct Inputs(Vec<ManifestInput>);

impl Inputs {
    /// Builds the input list, rejecting an empty one.
    ///
    /// # Errors
    /// Returns [`ManifestError::NoInputs`] when `inputs` is empty.
    pub fn new(inputs: Vec<ManifestInput>) -> Result<Inputs> {
        if inputs.is_empty() {
            Err(ManifestError::NoInputs)
        } else {
            Ok(Inputs(inputs))
        }
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
    use std::path::PathBuf;

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

    #[test]
    fn a_populated_list_derefs_to_its_inputs() {
        let inputs = Inputs::new(vec![an_input()]).unwrap();

        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs.first(), Some(&an_input()));
    }
}
