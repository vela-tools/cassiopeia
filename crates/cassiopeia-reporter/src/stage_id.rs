//! Typed identity and display name for a progress stage.

use derive_more::{AsRef, Display, From};

/// Stable identity of a progress stage, used as the reporter's internal key.
///
/// Stage identifiers are always known at compile time, so the newtype wraps a `&'static str` and is
/// cheap to copy and hash. It keeps a stage's identity from being confused with an unrelated string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, From, AsRef)]
pub struct StageId(#[as_ref(str)] &'static str);

impl StageId {
    /// Wraps a static stage identifier.
    #[must_use]
    pub const fn new(id: &'static str) -> StageId {
        StageId(id)
    }

    /// Returns the underlying static identifier.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

/// Human-readable display name of a progress stage, shown in the terminal backend.
///
/// Distinct from [`StageId`]: the label is what a reader sees, the id is what the reporter keys on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, From, AsRef)]
pub struct StageLabel(#[as_ref(str)] &'static str);

impl StageLabel {
    /// Wraps a static stage label.
    #[must_use]
    pub const fn new(label: &'static str) -> StageLabel {
        StageLabel(label)
    }

    /// Returns the underlying static label.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::stage_id::{StageId, StageLabel};

    #[test]
    fn a_stage_id_displays_and_derefs_to_its_inner_string() {
        let id = StageId::new("writer");

        assert_eq!(id.to_string(), "writer");
        assert_eq!(id.as_ref() as &str, "writer");
    }

    #[test]
    fn two_ids_with_the_same_text_are_equal() {
        assert_eq!(StageId::new("writer"), StageId::from("writer"));
    }

    #[test]
    fn a_stage_label_displays_its_inner_string() {
        assert_eq!(StageLabel::new("Writer").to_string(), "Writer");
    }
}
