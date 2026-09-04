/// Which screen of the mapping wizard is showing. The wizard is a linear flow with two branches: a
/// path for an existing model, and a path for naming a brand-new one.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WizardStep {
    /// Choosing an existing model or the create-new action.
    ModelSelection,
    /// Typing the name of a brand-new data model.
    DataModelNameInput,
    /// Entering the entity-id template on the identity form.
    IdentityForm,
    /// Browsing and editing the attribute tree.
    AttributeList,
    /// Editing one attribute's fields.
    AttributeEditor,
    /// Previewing and saving the finished mapping.
    SavePreview,
}

#[cfg(test)]
mod tests {
    use crate::screen::wizard::step::WizardStep;

    #[test]
    fn steps_compare_by_value() {
        assert_eq!(WizardStep::ModelSelection, WizardStep::ModelSelection);
        assert_ne!(WizardStep::ModelSelection, WizardStep::SavePreview);
    }
}
