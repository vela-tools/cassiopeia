use crate::environment::variable_name::VariableName;
use derive_where::derive_where;
use percent_encoding::{AsciiSet, CONTROLS, NON_ALPHANUMERIC, utf8_percent_encode};
use secrecy::{ExposeSecret, SecretString};
use std::{borrow::Cow, cmp::Reverse};

/// The characters the `url` crate percent-encodes in a query (WHATWG URL Standard, "query
/// percent-encode set").
const QUERY: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'<').add(b'>');

/// The query set plus the apostrophe, used for the query of a special scheme such as `https`.
const SPECIAL_QUERY: &AsciiSet = &QUERY.add(b'\'');

/// The characters the `url` crate percent-encodes in a path segment ("path percent-encode set").
const PATH: &AsciiSet = &QUERY.add(b'?').add(b'^').add(b'`').add(b'{').add(b'}');

/// The characters the `url` crate percent-encodes in a username or password ("userinfo
/// percent-encode set").
const USERINFO: &AsciiSet = &PATH.add(b'/').add(b':').add(b';').add(b'=').add(b'@').add(b'[').add(b'\\').add(b']').add(b'|');

/// Every percent-encode set a value read into a URL may have been written in, so a value that lands
/// in any component of a URL is still recognised there.
const ENCODINGS: [&AsciiSet; 5] = [QUERY, SPECIAL_QUERY, PATH, USERINFO, NON_ALPHANUMERIC];

/// The opening of a template placeholder, which a literal must escape.
const PLACEHOLDER_OPENING: &str = "${";

/// The escaped form of a literal `${` inside a template.
const ESCAPED_OPENING: &str = "$${";

/// A value read from the environment, held as a secret and named by the variable it came from.
///
/// It keeps every form the value can take once written into a URL, the value as read and each of
/// its percent-encodings, so masking finds it wherever it landed. Equality and hashing go by the
/// variable name alone: a process reads one value per name, and the forms are never compared.
#[derive(Clone)]
#[derive_where(Debug, PartialEq, Eq, Hash)]
pub struct Secret {
    /// The variable the value was read from, which a masked text shows in its place.
    name: VariableName,
    /// The value as read, followed by each distinct percent-encoding of it.
    #[derive_where(skip)]
    forms: Vec<SecretString>,
}

impl Secret {
    /// Holds `value`, read from the variable `name`, as a secret.
    #[must_use]
    pub fn new(name: VariableName, value: &SecretString) -> Secret {
        let raw = value.expose_secret();
        let mut forms: Vec<String> = Vec::with_capacity(ENCODINGS.len() + 1);
        forms.push(raw.to_owned());
        for encoding in ENCODINGS {
            let encoded = utf8_percent_encode(raw, encoding).to_string();
            if !forms.contains(&encoded) {
                forms.push(encoded);
            }
        }
        Secret {
            name,
            forms: forms.into_iter().map(SecretString::from).collect(),
        }
    }

    /// Returns the variable the value was read from.
    #[must_use]
    pub const fn name(&self) -> &VariableName {
        &self.name
    }
}

/// How a masked text renders what is not a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskStyle {
    /// Copy the rest of the text as it is, for a person to read.
    Plain,
    /// Escape every literal `${` as `$${`, so the result reads back as the same template.
    Template,
}

/// The secrets one value was built from, or that a process has read, each kept once.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Secrets(Vec<Secret>);

impl Secrets {
    /// Builds an empty set.
    #[must_use]
    pub const fn new() -> Secrets {
        Secrets(Vec::new())
    }

    /// Returns whether the set holds no secret.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates over the secrets in the order they were first added.
    pub fn iter(&self) -> impl Iterator<Item = &Secret> {
        self.0.iter()
    }

    /// Adds `secret` unless a secret of the same variable is already held.
    pub fn insert(&mut self, secret: Secret) {
        if !self.0.contains(&secret) {
            self.0.push(secret);
        }
    }

    /// Adds every secret of `other` that is not already held.
    pub fn extend(&mut self, other: &Secrets) {
        for secret in &other.0 {
            if !self.0.contains(secret) {
                // The set keeps its own copy, while `other` stays with the value it describes.
                self.0.push(secret.clone());
            }
        }
    }

    /// Renders `text` with every occurrence of a held secret, in any of its forms, replaced by
    /// `${NAME}`, the placeholder of the variable it was read from.
    ///
    /// Where two forms overlap, the longest one at a position wins, so a secret that contains
    /// another is masked whole. Text that holds no secret, in the plain style, is returned borrowed.
    #[must_use]
    pub fn mask<'text>(&self, text: &'text str, style: MaskStyle) -> Cow<'text, str> {
        let mut forms: Vec<(&str, &VariableName)> = self
            .0
            .iter()
            .flat_map(|secret| secret.forms.iter().map(move |form| (form.expose_secret(), &secret.name)))
            .filter(|(form, _)| !form.is_empty())
            .collect();
        if forms.is_empty() && (style == MaskStyle::Plain || !text.contains(PLACEHOLDER_OPENING)) {
            return Cow::Borrowed(text);
        }
        forms.sort_by_key(|(form, _)| Reverse(form.len()));
        let forms = forms;

        let mut masked = String::with_capacity(text.len());
        let mut changed = false;
        let mut rest = text;
        while let Some(character) = rest.chars().next() {
            if let Some((form, name)) = forms.iter().find(|(form, _)| rest.starts_with(form)) {
                masked.push_str(PLACEHOLDER_OPENING);
                masked.push_str(name.as_str());
                masked.push('}');
                rest = &rest[form.len()..];
                changed = true;
            } else if style == MaskStyle::Template && rest.starts_with(PLACEHOLDER_OPENING) {
                masked.push_str(ESCAPED_OPENING);
                rest = &rest[PLACEHOLDER_OPENING.len()..];
                changed = true;
            } else {
                masked.push(character);
                rest = &rest[character.len_utf8()..];
            }
        }

        if changed { Cow::Owned(masked) } else { Cow::Borrowed(text) }
    }
}

#[cfg(test)]
mod tests {
    use crate::environment::{
        secret::{MaskStyle, Secret, Secrets},
        variable_name::VariableName,
    };
    use pretty_assertions::assert_eq;
    use secrecy::SecretString;
    use std::{borrow::Cow, str::FromStr};

    fn secret(name: &str, value: &str) -> Secret {
        Secret::new(VariableName::from_str(name).unwrap(), &SecretString::from(value))
    }

    fn secrets(entries: &[(&str, &str)]) -> Secrets {
        let mut set = Secrets::new();
        for (name, value) in entries {
            set.insert(secret(name, value));
        }
        set
    }

    #[test]
    fn a_secret_is_replaced_by_its_placeholder() {
        let set = secrets(&[("API_KEY", "abc123")]);

        assert_eq!(
            set.mask("https://api.example/?key=abc123", MaskStyle::Plain),
            "https://api.example/?key=${API_KEY}"
        );
    }

    #[test]
    fn a_percent_encoded_secret_is_replaced_too() {
        let set = secrets(&[("TOKEN", "a b#c")]);

        assert_eq!(
            set.mask("https://api.example/?t=a%20b%23c", MaskStyle::Plain),
            "https://api.example/?t=${TOKEN}"
        );
    }

    #[test]
    fn the_longest_overlapping_secret_is_masked_whole() {
        let set = secrets(&[("SHORT", "abc"), ("LONG", "abcdef")]);

        assert_eq!(set.mask("x=abcdef y=abc", MaskStyle::Plain), "x=${LONG} y=${SHORT}");
    }

    #[test]
    fn text_without_a_secret_is_returned_borrowed() {
        let set = secrets(&[("API_KEY", "abc123")]);

        assert!(matches!(set.mask("nothing to hide", MaskStyle::Plain), Cow::Borrowed(_)));
    }

    #[test]
    fn an_empty_value_masks_nothing() {
        let set = secrets(&[("EMPTY", "")]);

        assert_eq!(set.mask("plain text", MaskStyle::Plain), "plain text");
    }

    #[test]
    fn the_template_style_escapes_a_literal_placeholder_opening() {
        let set = secrets(&[("API_KEY", "abc123")]);

        assert_eq!(set.mask("${literal}?key=abc123", MaskStyle::Template), "$${literal}?key=${API_KEY}");
        assert_eq!(Secrets::new().mask("a ${b}", MaskStyle::Template), "a $${b}");
    }

    #[test]
    fn the_plain_style_keeps_a_literal_placeholder_opening() {
        assert_eq!(Secrets::new().mask("a ${b}", MaskStyle::Plain), "a ${b}");
    }

    #[test]
    fn a_set_keeps_one_secret_per_variable() {
        let mut set = secrets(&[("API_KEY", "abc123")]);
        set.extend(&secrets(&[("API_KEY", "abc123"), ("OTHER", "xyz")]));

        assert_eq!(set.iter().map(|secret| secret.name().as_str()).collect::<Vec<_>>(), vec!["API_KEY", "OTHER"]);
    }

    #[test]
    fn the_debug_output_never_contains_the_value() {
        let rendered = format!("{:?}", secret("API_KEY", "supersecret"));

        assert!(!rendered.contains("supersecret"));
        assert!(rendered.contains("API_KEY"));
    }
}
