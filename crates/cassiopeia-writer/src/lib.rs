//! Writing stage for the Cassiopeia pipeline.
//!
//! The writer is the pipeline's sink: it takes finished [`NgsiLdEntity`](cassiopeia_ngsi_ld::entity::NgsiLdEntity)
//! values and emits them to a destination. Two destinations are provided: a
//! [`FileWriter`](file::file_writer::FileWriter) that streams one file per entity type, and a
//! [`BrokerWriter`](broker::broker_writer::BrokerWriter) that POSTs batches to an NGSI-LD Context
//! Broker under adaptive congestion control.

pub mod broker;
pub mod context_delivery;
pub mod error;
pub mod file;
pub mod run_outcome;
pub mod writer;

#[cfg(test)]
mod tests {
    use crate::{
        context_delivery::ContextDelivery,
        file::file_writer::{FileWriter, FileWriterConfig, Parallelism},
        run_outcome::RunOutcome,
        writer::Writer,
    };
    use cassiopeia_common::{file_framing::FileFraming, representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, context::ContextSource, name::NameBuf};
    use std::num::NonZeroUsize;
    use tempfile::TempDir;
    use urn_rs::Urn;

    #[test]
    fn the_public_surface_writes_an_entity_to_a_file() {
        let directory = TempDir::new().unwrap();
        let mut writer = FileWriter::new(FileWriterConfig {
            output_dir: directory.path().to_path_buf(),
            representation: NgsiLdRepresentation::Normalized,
            skip_null: NgsiLdSkipNull::Skip,
            parallelism: Parallelism::Sequential,
            context: ContextSource::None,
            context_delivery: ContextDelivery::Body,
            framing: FileFraming::Array,
            batch_size: NonZeroUsize::MIN,
        })
        .unwrap();

        writer
            .write(NgsiLdEntity::new(
                "urn:ngsi-ld:Sensor:1".parse::<Urn>().unwrap(),
                NameBuf::new("Sensor").unwrap(),
            ))
            .unwrap();
        let stats = writer.finalize(RunOutcome::Committed).unwrap();

        assert_eq!(stats.written, 1);
        assert!(directory.path().join("Sensor.json").exists());
    }
}
