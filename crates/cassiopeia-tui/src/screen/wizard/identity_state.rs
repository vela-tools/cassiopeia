/// The identity form's editing state: the entity-id template the user is typing and which field of
/// the form holds focus.
#[derive(Debug, Clone, Default)]
pub struct IdentityState {
    /// The entity-id template being typed.
    pub entity_id_template: String,
    /// Which form field holds focus.
    pub focus_index: usize,
}

#[cfg(test)]
mod tests {
    use crate::screen::wizard::identity_state::IdentityState;

    #[test]
    fn a_new_identity_state_is_empty_and_focused_on_the_first_field() {
        let state = IdentityState::default();
        assert!(state.entity_id_template.is_empty());
        assert_eq!(state.focus_index, 0);
    }
}
