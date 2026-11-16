use strum::Display;

/// A member of a `sensorthings` block that selects the entities of one collection by an `OData`
/// `$filter` expression, displayed as a manifest writes it, so a refusal can name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
#[strum(serialize_all = "camelCase")]
pub enum FilterSetting {
    /// `datastreams`, which selects Datastreams.
    Datastreams,
    /// `multiDatastreams`, which selects `MultiDatastreams`.
    MultiDatastreams,
    /// `things`, which selects the Things a latest read lists by themselves.
    Things,
}

impl FilterSetting {
    /// A filter of this setting, as an example a message can quote: a Datastream has one
    /// `ObservedProperty` to filter by, while a `MultiDatastream` has several, and a Thing has none.
    #[must_use]
    pub const fn example(self) -> &'static str {
        match self {
            FilterSetting::Datastreams => "ObservedProperty/name eq 'pm10'",
            FilterSetting::MultiDatastreams => "startswith(name,'ws-')",
            FilterSetting::Things => "startswith(name,'ILM_')",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::filter_setting::FilterSetting;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_setting_is_displayed_as_a_manifest_writes_it() {
        assert_eq!(FilterSetting::Datastreams.to_string(), "datastreams");
        assert_eq!(FilterSetting::MultiDatastreams.to_string(), "multiDatastreams");
        assert_eq!(FilterSetting::Things.to_string(), "things");
    }

    #[test]
    fn each_setting_quotes_a_filter_over_its_own_collection() {
        assert_eq!(FilterSetting::Datastreams.example(), "ObservedProperty/name eq 'pm10'");
        assert_eq!(FilterSetting::MultiDatastreams.example(), "startswith(name,'ws-')");
        assert_eq!(FilterSetting::Things.example(), "startswith(name,'ILM_')");
    }
}
