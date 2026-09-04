/// What the wizard should do if the user accepts a validation-warning popup.
#[derive(Debug, Clone)]
pub enum PopupAction {
    /// Use the non-PascalCase model name the user typed anyway.
    ForceModelName,

    /// Use the non-camelCase attribute name the user typed anyway.
    ForceAttributeName,
}

/// A modal validation warning: a name did not match the expected casing, and the user may accept
/// their input or take the suggested rewrite.
#[derive(Debug, Clone)]
pub struct WizardPopup {
    /// The popup's title line.
    pub title: String,
    /// The warning message body.
    pub message: String,
    /// The suggested casing-corrected rewrite of the user's input.
    pub suggestion: String,
    /// What accepting the popup does.
    pub action: PopupAction,
}

#[cfg(test)]
mod tests {
    use crate::screen::wizard::popup::{PopupAction, WizardPopup};

    #[test]
    fn a_popup_carries_its_message_and_action() {
        let popup = WizardPopup {
            title: "Validation Warning".to_string(),
            message: "not camelCase".to_string(),
            suggestion: "myField".to_string(),
            action: PopupAction::ForceAttributeName,
        };
        assert!(matches!(popup.action, PopupAction::ForceAttributeName));
        assert_eq!(popup.suggestion, "myField");
    }
}
