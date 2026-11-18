use crate::{
    observed_declaration::ObservedDeclaration,
    template::{CompiledTemplate, TemplateSource},
};
use serde::{Deserialize, Serialize};

/// The position of one distinct `observedAt` template among those a mapping declares, in the order
/// the mapping first declares them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ObservedTemplateIndex(usize);

impl ObservedTemplateIndex {
    /// The template at `index`.
    #[must_use]
    pub const fn new(index: usize) -> ObservedTemplateIndex {
        ObservedTemplateIndex(index)
    }

    /// The position of the template.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// One distinct `observedAt` template a mapping declares, compiled once, with every declaration
/// reading it.
///
/// Templates are told apart by their source text: two declarations that spell one template read the
/// same value off every record, so it is rendered once for both.
#[derive(Debug, Clone)]
pub struct ObservedTemplate {
    /// The template as written.
    source: TemplateSource,
    /// The template, compiled.
    compiled: CompiledTemplate,
    /// Every `observedAt` sub-attribute whose template this is.
    declarations: Vec<ObservedDeclaration>,
}

impl ObservedTemplate {
    /// The template `source`, compiled as `compiled`, read by `declarations`.
    #[must_use]
    pub const fn new(source: TemplateSource, compiled: CompiledTemplate, declarations: Vec<ObservedDeclaration>) -> ObservedTemplate {
        ObservedTemplate {
            source,
            compiled,
            declarations,
        }
    }

    /// The template as written.
    #[must_use]
    pub const fn source(&self) -> &TemplateSource {
        &self.source
    }

    /// The template, compiled.
    #[must_use]
    pub const fn compiled(&self) -> &CompiledTemplate {
        &self.compiled
    }

    /// Every `observedAt` sub-attribute whose template this is.
    #[must_use]
    pub fn declarations(&self) -> &[ObservedDeclaration] {
        &self.declarations
    }
}

#[cfg(test)]
mod tests {
    use crate::observed_template::ObservedTemplateIndex;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_index_names_the_position_it_was_made_with() {
        assert_eq!(ObservedTemplateIndex::new(2).get(), 2);
    }

    #[test]
    fn indices_order_by_position() {
        assert!(ObservedTemplateIndex::new(0) < ObservedTemplateIndex::new(1));
    }

    #[test]
    fn an_index_survives_a_round_trip_through_its_wire_form() {
        let index = ObservedTemplateIndex::new(3);

        assert_eq!(
            serde_json::from_value::<ObservedTemplateIndex>(serde_json::to_value(index).unwrap()).unwrap(),
            index
        );
    }
}
