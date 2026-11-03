use serde::{Deserialize, Serialize};

/// What a manifest's `header` member states: whether the table has a header at all.
///
/// The member is a JSON boolean on the wire, `true` for [`HeaderFlag::Present`] and `false` for
/// [`HeaderFlag::Absent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "bool", into = "bool")]
pub enum HeaderFlag {
    /// The table has a header (`header: true`).
    Present,
    /// The table has no header (`header: false`).
    Absent,
}

// Written by hand: no derive maps a `bool` onto the variants of a two-variant enum.
impl From<bool> for HeaderFlag {
    fn from(present: bool) -> HeaderFlag {
        if present { HeaderFlag::Present } else { HeaderFlag::Absent }
    }
}

// Written by hand: no derive maps the variants of a two-variant enum onto a `bool`.
impl From<HeaderFlag> for bool {
    fn from(flag: HeaderFlag) -> bool {
        match flag {
            HeaderFlag::Present => true,
            HeaderFlag::Absent => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::header_flag::HeaderFlag;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn a_json_boolean_reads_as_presence_or_absence() {
        assert_eq!(serde_json::from_value::<HeaderFlag>(json!(true)).unwrap(), HeaderFlag::Present);
        assert_eq!(serde_json::from_value::<HeaderFlag>(json!(false)).unwrap(), HeaderFlag::Absent);
    }

    #[test]
    fn a_flag_is_written_back_as_a_json_boolean() {
        assert_eq!(serde_json::to_value(HeaderFlag::Present).unwrap(), json!(true));
        assert_eq!(serde_json::to_value(HeaderFlag::Absent).unwrap(), json!(false));
    }

    #[test]
    fn anything_but_a_boolean_is_rejected() {
        assert!(serde_json::from_value::<HeaderFlag>(json!("true")).is_err());
    }
}
