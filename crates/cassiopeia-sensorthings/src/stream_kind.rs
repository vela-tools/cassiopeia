use strum::Display;

/// The kind of a SensorThings data stream: a Datastream (OGC 18-088 section 8.2.4), whose
/// Observations each hold one result, or a `MultiDatastream` of the `MultiDatastream` extension
/// (section 12), whose Observations each hold one result per `ObservedProperty`.
///
/// Kinds are ordered Datastreams first, which decides the order a range read requests data streams
/// in and which reading keeps a name several readings of one Thing share. A kind is displayed in
/// kebab case, `datastream` or `multi-datastream`, as the command line writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum StreamKind {
    /// A Datastream.
    Datastream,
    /// A `MultiDatastream`.
    MultiDatastream,
}

impl StreamKind {
    /// The name of the entity type, as a message names one: `Datastream` or `MultiDatastream`.
    #[must_use]
    pub const fn entity(self) -> &'static str {
        match self {
            StreamKind::Datastream => "Datastream",
            StreamKind::MultiDatastream => "MultiDatastream",
        }
    }

    /// The collection a server serves the data streams of this kind in, as its service root lists it
    /// (OGC 18-088 section 9.2.1).
    #[must_use]
    pub const fn collection(self) -> &'static str {
        match self {
            StreamKind::Datastream => "Datastreams",
            StreamKind::MultiDatastream => "MultiDatastreams",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::stream_kind::StreamKind;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_kind_is_displayed_in_kebab_case_and_names_its_entity_and_collection() {
        assert_eq!(StreamKind::Datastream.to_string(), "datastream");
        assert_eq!(StreamKind::MultiDatastream.to_string(), "multi-datastream");
        assert_eq!(StreamKind::MultiDatastream.entity(), "MultiDatastream");
        assert_eq!(StreamKind::MultiDatastream.collection(), "MultiDatastreams");
        assert_eq!(StreamKind::Datastream.collection(), "Datastreams");
    }

    #[test]
    fn datastreams_come_before_multi_datastreams() {
        assert!(StreamKind::Datastream < StreamKind::MultiDatastream);
    }
}
