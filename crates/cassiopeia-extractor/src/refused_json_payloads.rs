use crate::refused_json_payload::RefusedJsonPayload;
use cassiopeia_batch_sink::{batch_sink::BatchSink, occurrences::Occurrences};
use cassiopeia_ngsi_ld::{entity::name::NameBuf, value::json_payload_error::JsonPayloadError};
use derive_more::{Deref, Into};

/// The `JsonProperty` values one batch refused, one count per distinct refusal of one attribute.
#[derive(Default, Deref, Into)]
pub struct RefusedJsonPayloads(BatchSink<RefusedJsonPayload, Occurrences>);

impl RefusedJsonPayloads {
    /// Opens an empty sink.
    #[must_use]
    pub fn new() -> RefusedJsonPayloads {
        RefusedJsonPayloads::default()
    }

    /// Records one refusal, counting a repeat of one already seen.
    pub fn record(&self, attribute: &NameBuf, refusal: JsonPayloadError) {
        // The key carries the refusal by value, so it is built owned on every refusal; this runs on
        // the refusal path only.
        let refused = RefusedJsonPayload {
            attribute: attribute.clone(),
            refusal,
        };

        self.0.tally_owned(refused, Occurrences::first);
    }
}

#[cfg(test)]
mod tests {
    use crate::refused_json_payloads::RefusedJsonPayloads;
    use cassiopeia_batch_sink::{batch_sink::BatchSink, recurring::Recurring};
    use cassiopeia_ngsi_ld::{
        entity::name::NameBuf,
        value::json_payload_error::{JsonPayloadError, JsonShape},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn repeats_of_one_refusal_are_counted_rather_than_listed() {
        let name = NameBuf::new("payload").expect("valid name");
        let sink = RefusedJsonPayloads::new();
        sink.record(&name, JsonPayloadError::NotAnObject { found: JsonShape::Number });
        sink.record(&name, JsonPayloadError::NotAnObject { found: JsonShape::Number });
        sink.record(&name, JsonPayloadError::NotAnObject { found: JsonShape::String });

        let counts = BatchSink::from(sink)
            .into_entries()
            .map(|(_, record)| record.occurrences().get())
            .collect::<Vec<u64>>();
        assert_eq!(counts, [2, 1]);
    }
}
