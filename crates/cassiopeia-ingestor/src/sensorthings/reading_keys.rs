use cassiopeia_sensorthings::entity_id::EntityId;
use derive_more::{Display, From};
use std::collections::BTreeMap;

/// The key a reading is found under in a record's `readings`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, From)]
pub struct ReadingKey(Box<str>);

/// A data stream whose reading is keyed `<name>@<data stream id>`, because a data stream of the same
/// Thing with a lower id keeps the `ObservedProperty`'s name.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
#[display("Thing {thing} reads data stream {datastream} as `readings['{key}']`")]
pub struct RenamedReading {
    /// The Thing both data streams belong to.
    pub thing: EntityId,
    /// The renamed data stream.
    pub datastream: EntityId,
    /// The key its readings are found under.
    pub key: ReadingKey,
}

impl ReadingKey {
    /// The key as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The keys of one Thing's data streams, given as each one's id and `ObservedProperty` name, in the
/// order given.
///
/// A reading's key is its `ObservedProperty`'s name. When several data streams of the Thing share a
/// name, the one with the lowest id keeps it and each other is keyed `<name>@<data stream id>`, as
/// CSV keys a repeated column `<name>@<column number>`. The keys depend only on the data streams, so
/// a data stream has the same key in every read mode.
pub(super) fn reading_keys<'a>(datastreams: &[(&'a EntityId, &'a str)]) -> Vec<ReadingKey> {
    let mut keepers: BTreeMap<&str, &EntityId> = BTreeMap::new();
    for &(id, name) in datastreams {
        keepers.entry(name).and_modify(|keeper| *keeper = (*keeper).min(id)).or_insert(id);
    }
    datastreams
        .iter()
        .map(|&(id, name)| match keepers.get(name) {
            Some(&keeper) if keeper == id => ReadingKey(name.into()),
            Some(_) | None => ReadingKey(format!("{name}@{id}").into()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::sensorthings::reading_keys::{ReadingKey, RenamedReading, reading_keys};
    use cassiopeia_sensorthings::entity_id::EntityId;
    use pretty_assertions::assert_eq;

    fn keys(datastreams: &[(EntityId, &str)]) -> Vec<String> {
        let borrowed: Vec<(&EntityId, &str)> = datastreams.iter().map(|(id, name)| (id, *name)).collect();
        reading_keys(&borrowed).iter().map(ToString::to_string).collect()
    }

    #[test]
    fn distinct_names_are_their_own_keys() {
        assert_eq!(
            keys(&[(EntityId::Integer(56776), "pm10"), (EntityId::Integer(56775), "pm25_kal")]),
            ["pm10", "pm25_kal"]
        );
    }

    #[test]
    fn the_lowest_data_stream_id_keeps_a_repeated_name() {
        assert_eq!(keys(&[(EntityId::Integer(9), "pm10"), (EntityId::Integer(5), "pm10")]), ["pm10@9", "pm10"]);
    }

    #[test]
    fn integer_ids_keep_a_name_before_text_ids() {
        assert_eq!(
            keys(&[(EntityId::Text("a".into()), "pm10"), (EntityId::Integer(10), "pm10")]),
            ["pm10@a", "pm10"]
        );
    }

    #[test]
    fn a_rename_is_written_as_the_bracketed_key_a_template_reads() {
        let renamed = RenamedReading {
            thing: EntityId::Integer(12512),
            datastream: EntityId::Integer(9),
            key: ReadingKey("pm10@9".into()),
        };

        assert_eq!(renamed.to_string(), "Thing 12512 reads data stream 9 as `readings['pm10@9']`");
    }
}
