use crate::environment::secret::{MaskStyle, Secrets};
use parking_lot::RwLock;
use std::borrow::Cow;

/// Every secret this process has read from the environment.
///
/// A value read into a URL or a header is masked by the type that carries it, but a library's own
/// error text or a log line can still quote it. Every output sink masks against this set as a
/// backstop, so a secret that slips past its type is still never written out.
static READ: RwLock<Secrets> = RwLock::new(Secrets::new());

/// Records `secrets` as read, so every output sink masks them from then on.
pub fn register(secrets: &Secrets) {
    if !secrets.is_empty() {
        READ.write().extend(secrets);
    }
}

/// Renders `text` for output with every secret this process has read replaced by its placeholder.
#[must_use]
pub fn mask(text: &str) -> Cow<'_, str> {
    READ.read().mask(text, MaskStyle::Plain)
}

#[cfg(test)]
mod tests {
    use crate::environment::{
        registry::{mask, register},
        secret::{Secret, Secrets},
        variable_name::VariableName,
    };
    use pretty_assertions::assert_eq;
    use secrecy::SecretString;
    use std::str::FromStr;

    #[test]
    fn a_registered_secret_is_masked_in_any_later_text() {
        let mut secrets = Secrets::new();
        secrets.insert(Secret::new(
            VariableName::from_str("REGISTRY_TEST_TOKEN").unwrap(),
            &SecretString::from("registry-test-value-7f3a"),
        ));

        register(&secrets);

        assert_eq!(
            mask("request to https://x/?t=registry-test-value-7f3a failed"),
            "request to https://x/?t=${REGISTRY_TEST_TOKEN} failed"
        );
    }

    #[test]
    fn text_holding_no_registered_secret_is_unchanged() {
        assert_eq!(mask("an unrelated line"), "an unrelated line");
    }
}
