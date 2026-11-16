use crate::sensorthings::element_position::ElementPosition;
use cassiopeia_sensorthings::entity_id::EntityId;
use derive_more::{Display, From};
use std::collections::BTreeMap;

/// The key a reading is found under in a record's `readings`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, From)]
pub struct ReadingKey(Box<str>);

/// Where a reading of a Thing comes from: a Datastream, or one element of a `MultiDatastream`.
///
/// Sources are ordered as reading keys rank them: Datastreams before `MultiDatastream` elements, then
/// by id, then by position.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Display)]
pub enum ReadingSource {
    /// The Datastream with this id.
    #[display("Datastream {_0}")]
    Datastream(EntityId),

    /// The element at a position of the `MultiDatastream` with this id.
    #[display("element {_1} of MultiDatastream {_0}")]
    Element(EntityId, ElementPosition),
}

/// A reading keyed `<name>@<data stream id>` or `<name>@<MultiDatastream id>/<position>`, because a
/// reading of the same Thing that ranks before it keeps the `ObservedProperty`'s name.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
#[display("Thing {thing} reads {source} as `readings['{key}']`")]
pub struct RenamedReading {
    /// The Thing both readings belong to.
    pub thing: EntityId,
    /// Where the renamed reading comes from.
    pub source: ReadingSource,
    /// The key the reading is found under.
    pub key: ReadingKey,
}

impl ReadingKey {
    /// The key as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The keys of one Thing's readings, given as each one's source and `ObservedProperty` name, in the
/// order given.
///
/// A reading's key is its `ObservedProperty`'s name. When several readings of the Thing share a name,
/// the one whose source ranks first keeps it: Datastreams before `MultiDatastream` elements, then the
/// lowest id, then the lowest position. Each other is keyed `<name>@<data stream id>` for a Datastream
/// and `<name>@<MultiDatastream id>/<position>` for an element, as CSV keys a repeated column
/// `<name>@<column number>`. The keys depend only on the data streams, so a reading has the same key
/// in every read mode.
pub(super) fn reading_keys<'a>(readings: &[(&'a ReadingSource, &'a str)]) -> Vec<ReadingKey> {
    let mut keepers: BTreeMap<&str, &ReadingSource> = BTreeMap::new();
    for &(source, name) in readings {
        keepers.entry(name).and_modify(|keeper| *keeper = (*keeper).min(source)).or_insert(source);
    }
    readings
        .iter()
        .map(|&(source, name)| match (keepers.get(name), source) {
            (Some(&keeper), _) if keeper == source => ReadingKey(name.into()),
            (Some(_) | None, ReadingSource::Datastream(id)) => ReadingKey(format!("{name}@{id}").into()),
            (Some(_) | None, ReadingSource::Element(id, position)) => ReadingKey(format!("{name}@{id}/{position}").into()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::{
        element_position::ElementPosition,
        reading_keys::{ReadingKey, ReadingSource, RenamedReading, reading_keys},
    };
    use cassiopeia_sensorthings::entity_id::EntityId;
    use pretty_assertions::assert_eq;

    fn keys(readings: &[(ReadingSource, &str)]) -> Vec<String> {
        let borrowed: Vec<(&ReadingSource, &str)> = readings.iter().map(|(source, name)| (source, *name)).collect();
        reading_keys(&borrowed).iter().map(ToString::to_string).collect()
    }

    fn datastream(id: i64) -> ReadingSource {
        ReadingSource::Datastream(EntityId::Integer(id))
    }

    fn element(id: i64, index: usize) -> ReadingSource {
        ReadingSource::Element(EntityId::Integer(id), ElementPosition::at_index(index))
    }

    #[test]
    fn distinct_names_are_their_own_keys() {
        assert_eq!(keys(&[(datastream(56776), "pm10"), (datastream(56775), "pm25_kal")]), ["pm10", "pm25_kal"]);
    }

    #[test]
    fn the_lowest_data_stream_id_keeps_a_repeated_name() {
        assert_eq!(keys(&[(datastream(9), "pm10"), (datastream(5), "pm10")]), ["pm10@9", "pm10"]);
    }

    #[test]
    fn integer_ids_keep_a_name_before_text_ids() {
        assert_eq!(
            keys(&[(ReadingSource::Datastream(EntityId::Text("a".into())), "pm10"), (datastream(10), "pm10")]),
            ["pm10@a", "pm10"]
        );
    }

    #[test]
    fn a_datastream_keeps_a_name_before_an_element_and_an_element_is_keyed_by_its_position() {
        assert_eq!(
            keys(&[(element(4, 0), "temperature"), (datastream(9), "temperature"), (element(4, 1), "humidity")]),
            ["temperature@4/1", "temperature", "humidity"]
        );
    }

    #[test]
    fn elements_rank_by_id_then_by_position() {
        assert_eq!(
            keys(&[(element(7, 0), "temperature"), (element(4, 2), "temperature"), (element(4, 1), "temperature")]),
            ["temperature@7/1", "temperature@4/3", "temperature"]
        );
    }

    #[test]
    fn a_rename_is_written_as_the_bracketed_key_a_template_reads() {
        let renamed = RenamedReading {
            thing: EntityId::Integer(12512),
            source: datastream(9),
            key: ReadingKey("pm10@9".into()),
        };
        let element = RenamedReading {
            thing: EntityId::Integer(1),
            source: element(4, 0),
            key: ReadingKey("temperature@4/1".into()),
        };

        assert_eq!(renamed.to_string(), "Thing 12512 reads Datastream 9 as `readings['pm10@9']`");
        assert_eq!(
            element.to_string(),
            "Thing 1 reads element 1 of MultiDatastream 4 as `readings['temperature@4/1']`"
        );
    }
}
