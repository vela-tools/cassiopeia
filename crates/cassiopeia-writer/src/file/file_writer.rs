use crate::{
    context_delivery::ContextDelivery,
    error::Result,
    file::entity_type_sink::EntityTypeSink,
    run_outcome::RunOutcome,
    writer::{Writer, WriterProgress, WriterStats},
};
use ahash::AHashMap;
use cassiopeia_common::{
    error::io::{IoAction, IoError},
    file_framing::FileFraming,
    representation::NgsiLdRepresentation,
    skip_null::NgsiLdSkipNull,
};
use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, context::ContextSource, name::NameBuf};
use rayon::prelude::*;
use std::{collections::hash_map::Entry, fs, mem, num::NonZeroUsize, path::PathBuf, time::Duration};

/// Whether a batch is written across threads or on the calling thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Parallelism {
    /// Write entities concurrently with a Rayon parallel iterator.
    #[default]
    Parallel,
    /// Write entities one at a time on the calling thread.
    Sequential,
}

/// Configuration for a [`FileWriter`].
pub struct FileWriterConfig {
    /// The directory the per-entity-type files are written into.
    pub output_dir: PathBuf,
    /// The NGSI-LD representation entities are serialized in.
    pub representation: NgsiLdRepresentation,
    /// Whether null-valued attributes are skipped during serialization.
    pub skip_null: NgsiLdSkipNull,
    /// Whether a batch is written in parallel or sequentially.
    pub parallelism: Parallelism,
    /// The `@context` to attach to each entity.
    pub context: ContextSource,
    /// How the `@context` is delivered; `LinkHeader` leaves it out of the file body.
    pub context_delivery: ContextDelivery,
    /// How each per-entity-type file is framed.
    pub framing: FileFraming,
    /// Maximum number of entities prepared in parallel before their bytes are streamed to disk.
    pub batch_size: NonZeroUsize,
}

/// Computes the file extension for the framing and whether the entity carries an `@context`.
///
/// The four extensions are the cross-product of the two axes: framing (array vs line-delimited) and
/// context presence (`@context` embedded in the body or not).
const fn file_extension(framing: FileFraming, has_context: bool) -> &'static str {
    match (framing, has_context) {
        (FileFraming::Array, false) => "json",
        (FileFraming::Array, true) => "jsonld",
        (FileFraming::LineDelimited, false) => "jsonl",
        (FileFraming::LineDelimited, true) => "ndjsonld",
    }
}

/// A streaming writer that emits one file per entity type.
///
/// Entities are retained only until one configured batch is full. Representation conversion and
/// serialization fan that bounded batch across Rayon, then the resulting bytes are appended in
/// input order. Memory is therefore proportional to the batch rather than the whole dataset.
pub struct FileWriter {
    output_dir: PathBuf,
    sinks: AHashMap<NameBuf, EntityTypeSink>,
    pending: Vec<NgsiLdEntity>,
    batch_size: NonZeroUsize,
    written_count: usize,
    failed_count: usize,
    bytes_written: u64,
    representation: NgsiLdRepresentation,
    skip_null: NgsiLdSkipNull,
    parallelism: Parallelism,
    context: ContextSource,
    framing: FileFraming,
    size_estimate: usize,
}

/// The per-entity output-buffer capacity seeded for the first batch, before any serialized sizes
/// have been observed.
const INITIAL_SIZE_ESTIMATE: usize = 256;

/// One entity after context resolution and JSON serialization, ready for ordered output.
struct PreparedEntity {
    entity_type: NameBuf,
    extension: &'static str,
    serialized: Vec<u8>,
}

impl FileWriter {
    /// Builds a file writer, creating the output directory if it does not yet exist.
    ///
    /// # Errors
    /// Returns a [`WriterError`] when the output path exists but is not a directory, or when the
    /// directory cannot be created.
    pub fn new(config: FileWriterConfig) -> Result<FileWriter> {
        if !config.output_dir.exists() {
            fs::create_dir_all(&config.output_dir).map_err(|source| IoError::DirectoryOperation {
                source,
                path: config.output_dir.clone(),
                action: IoAction::Create,
            })?;
        } else if !config.output_dir.is_dir() {
            return Err(IoError::NotADirectory { path: config.output_dir }.into());
        }

        // Link-header delivery keeps the `@context` out of the file body, so the writer resolves no
        // context at all in that mode.
        let context = match config.context_delivery {
            ContextDelivery::LinkHeader => ContextSource::None,
            ContextDelivery::Body => config.context,
        };

        Ok(FileWriter {
            output_dir: config.output_dir,
            sinks: AHashMap::new(),
            pending: Vec::with_capacity(config.batch_size.get()),
            batch_size: config.batch_size,
            written_count: 0,
            failed_count: 0,
            bytes_written: 0,
            representation: config.representation,
            skip_null: config.skip_null,
            parallelism: config.parallelism,
            context,
            framing: config.framing,
            size_estimate: INITIAL_SIZE_ESTIMATE,
        })
    }

    /// Resolves context and serializes one entity without touching shared writer state.
    fn prepare_entity(
        mut entity: NgsiLdEntity,
        context: &ContextSource,
        representation: NgsiLdRepresentation,
        skip_null: NgsiLdSkipNull,
        framing: FileFraming,
        capacity_hint: usize,
    ) -> Result<PreparedEntity> {
        if let Some(context) = context.resolve(&entity.entity_type) {
            entity.context = Some(context.clone());
        }

        let entity_type = entity.entity_type.clone();
        let extension = file_extension(framing, entity.context.is_some());
        let serialized = EntityTypeSink::serialize_entity(&entity, representation, skip_null, framing, capacity_hint)?;

        Ok(PreparedEntity {
            entity_type,
            extension,
            serialized,
        })
    }

    /// Converts a bounded batch into ordered serialized buffers, seeding each entity's output buffer
    /// from the running size estimate.
    fn prepare_batch(&self, entities: Vec<NgsiLdEntity>) -> Result<Vec<PreparedEntity>> {
        let hint = self.size_estimate;
        let prepare = |entity| Self::prepare_entity(entity, &self.context, self.representation, self.skip_null, self.framing, hint);
        match self.parallelism {
            Parallelism::Parallel => entities.into_par_iter().map(prepare).collect(),
            Parallelism::Sequential => entities.into_iter().map(prepare).collect(),
        }
    }

    /// Appends one prepared entity to its type sink, opening that sink on first use.
    fn write_prepared(&mut self, prepared: PreparedEntity) -> Result<()> {
        match self.sinks.entry(prepared.entity_type) {
            Entry::Occupied(mut occupied) => occupied.get_mut().write_serialized(&prepared.serialized)?,
            Entry::Vacant(vacant) => {
                let mut sink = EntityTypeSink::open(&self.output_dir, vacant.key(), prepared.extension, self.framing)?;
                sink.write_serialized(&prepared.serialized)?;
                vacant.insert(sink);
            }
        }

        self.written_count += 1;
        self.bytes_written = self.bytes_written.saturating_add(u64::try_from(prepared.serialized.len()).unwrap_or(u64::MAX));
        Ok(())
    }

    /// Serializes and drains the current bounded batch.
    fn flush_pending(&mut self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }

        let entities = mem::replace(&mut self.pending, Vec::with_capacity(self.batch_size.get()));
        let prepared = self.prepare_batch(entities)?;

        // Seed the next batch's buffers from the largest entity just serialized: entities of one type
        // are similarly sized, so the peak avoids reallocation without wildly over-allocating.
        if let Some(peak) = prepared.iter().map(|entity| entity.serialized.len()).max() {
            self.size_estimate = peak.max(INITIAL_SIZE_ESTIMATE);
        }

        for entity in prepared {
            self.write_prepared(entity)?;
        }
        Ok(())
    }
}

impl Writer for FileWriter {
    fn write(&mut self, entity: NgsiLdEntity) -> Result<()> {
        self.pending.push(entity);
        if self.pending.len() >= self.batch_size.get() {
            self.flush_pending()?;
        }
        Ok(())
    }

    fn write_batch(&mut self, entities: Vec<NgsiLdEntity>) -> Result<()> {
        for entity in entities {
            self.write(entity)?;
        }
        Ok(())
    }

    fn progress(&self) -> WriterProgress {
        WriterProgress {
            written: self.written_count,
            failed: self.failed_count,
            bytes_written: self.bytes_written,
        }
    }

    fn finalize(&mut self, _outcome: RunOutcome) -> Result<WriterStats> {
        self.flush_pending()?;
        let sinks: Vec<EntityTypeSink> = mem::take(&mut self.sinks).into_values().collect();

        match self.parallelism {
            Parallelism::Parallel => {
                let results: Vec<Result<()>> = sinks.into_par_iter().map(EntityTypeSink::finalize).collect();
                for result in results {
                    result?;
                }
            }
            Parallelism::Sequential => {
                for sink in sinks {
                    sink.finalize()?;
                }
            }
        }

        Ok(WriterStats {
            written: self.written_count,
            failed: self.failed_count,
            bytes_written: self.bytes_written,
            request_time: Duration::ZERO,
            retries: 0,
            backoff: Duration::ZERO,
            queue_wait: Duration::ZERO,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_delivery::ContextDelivery,
        error::WriterError,
        file::file_writer::{FileWriter, FileWriterConfig, Parallelism, file_extension},
        run_outcome::RunOutcome,
        writer::Writer,
    };
    use cassiopeia_common::{error::io::IoError, file_framing::FileFraming, representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
    use cassiopeia_ngsi_ld::entity::{
        NgsiLdEntity,
        context::{ContextSource, NgsiLdContext},
        name::NameBuf,
    };
    use std::{fs, num::NonZeroUsize};
    use tempfile::TempDir;
    use url::Url;
    use urn_rs::Urn;

    fn writer(directory: &TempDir) -> FileWriter {
        framed_writer(directory, FileFraming::Array, ContextSource::None)
    }

    fn framed_writer(directory: &TempDir, framing: FileFraming, context: ContextSource) -> FileWriter {
        FileWriter::new(FileWriterConfig {
            output_dir: directory.path().to_path_buf(),
            representation: NgsiLdRepresentation::Normalized,
            skip_null: NgsiLdSkipNull::Skip,
            parallelism: Parallelism::Sequential,
            context,
            context_delivery: ContextDelivery::Body,
            framing,
            batch_size: NonZeroUsize::new(2).unwrap(),
        })
        .unwrap()
    }

    fn parallel_writer(directory: &TempDir, batch_size: usize) -> FileWriter {
        FileWriter::new(FileWriterConfig {
            output_dir: directory.path().to_path_buf(),
            representation: NgsiLdRepresentation::Normalized,
            skip_null: NgsiLdSkipNull::Skip,
            parallelism: Parallelism::Parallel,
            context: ContextSource::None,
            context_delivery: ContextDelivery::Body,
            framing: FileFraming::Array,
            batch_size: NonZeroUsize::new(batch_size).unwrap(),
        })
        .unwrap()
    }

    fn entity(entity_type: &str, id: &str) -> NgsiLdEntity {
        NgsiLdEntity::new(id.parse::<Urn>().unwrap(), NameBuf::new(entity_type).unwrap())
    }

    #[test]
    fn one_file_is_written_per_entity_type() {
        let directory = TempDir::new().unwrap();
        let mut writer = writer(&directory);

        writer.write(entity("Sensor", "urn:ngsi-ld:Sensor:1")).unwrap();
        writer.write(entity("Sensor", "urn:ngsi-ld:Sensor:2")).unwrap();
        writer.write(entity("Device", "urn:ngsi-ld:Device:1")).unwrap();
        let stats = writer.finalize(RunOutcome::Committed).unwrap();

        assert_eq!(stats.written, 3);
        assert!(directory.path().join("Sensor.json").exists());
        assert!(directory.path().join("Device.json").exists());
    }

    #[test]
    fn a_finalized_file_is_a_json_array_of_its_entities() {
        let directory = TempDir::new().unwrap();
        let mut writer = writer(&directory);

        writer.write(entity("Sensor", "urn:ngsi-ld:Sensor:1")).unwrap();
        writer.write(entity("Sensor", "urn:ngsi-ld:Sensor:2")).unwrap();
        writer.finalize(RunOutcome::Committed).unwrap();

        let content = fs::read_to_string(directory.path().join("Sensor.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let array = parsed.as_array().unwrap();

        assert_eq!(array.len(), 2);
        assert_eq!(array[0]["id"], "urn:ngsi-ld:Sensor:1");
        assert_eq!(array[1]["type"], "Sensor");
    }

    #[test]
    fn parallel_preparation_stays_bounded_and_preserves_order() {
        let directory = TempDir::new().unwrap();
        let mut writer = parallel_writer(&directory, 2);

        writer.write(entity("Sensor", "urn:ngsi-ld:Sensor:1")).unwrap();
        assert_eq!(writer.pending.len(), 1);
        writer.write(entity("Sensor", "urn:ngsi-ld:Sensor:2")).unwrap();
        assert!(writer.pending.is_empty());
        assert_eq!(writer.written_count, 2);
        writer.write(entity("Sensor", "urn:ngsi-ld:Sensor:3")).unwrap();
        assert_eq!(writer.pending.len(), 1);

        let stats = writer.finalize(RunOutcome::Committed).unwrap();
        assert_eq!(stats.written, 3);

        let content = fs::read_to_string(directory.path().join("Sensor.json")).unwrap();
        let array: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        let ids: Vec<&str> = array.iter().filter_map(|value| value["id"].as_str()).collect();
        assert_eq!(ids, ["urn:ngsi-ld:Sensor:1", "urn:ngsi-ld:Sensor:2", "urn:ngsi-ld:Sensor:3"]);
    }

    #[test]
    fn a_missing_output_directory_is_created() {
        let parent = TempDir::new().unwrap();
        let nested = parent.path().join("nested/output");
        let mut writer = FileWriter::new(FileWriterConfig {
            output_dir: nested.clone(),
            representation: NgsiLdRepresentation::Normalized,
            skip_null: NgsiLdSkipNull::Skip,
            parallelism: Parallelism::Sequential,
            context: ContextSource::None,
            context_delivery: ContextDelivery::Body,
            framing: FileFraming::Array,
            batch_size: NonZeroUsize::new(2).unwrap(),
        })
        .unwrap();

        writer.write(entity("Sensor", "urn:ngsi-ld:Sensor:1")).unwrap();
        writer.finalize(RunOutcome::Committed).unwrap();

        assert!(nested.join("Sensor.json").exists());
    }

    #[test]
    fn an_output_path_that_is_a_file_is_rejected() {
        let directory = TempDir::new().unwrap();
        let file_path = directory.path().join("not-a-dir");
        fs::write(&file_path, "x").unwrap();

        let result = FileWriter::new(FileWriterConfig {
            output_dir: file_path,
            representation: NgsiLdRepresentation::Normalized,
            skip_null: NgsiLdSkipNull::Skip,
            parallelism: Parallelism::Sequential,
            context: ContextSource::None,
            context_delivery: ContextDelivery::Body,
            framing: FileFraming::Array,
            batch_size: NonZeroUsize::new(2).unwrap(),
        });

        assert!(matches!(result, Err(WriterError::Io(IoError::NotADirectory { .. }))));
    }

    #[test]
    fn the_extension_table_covers_both_axes() {
        assert_eq!(file_extension(FileFraming::Array, false), "json");
        assert_eq!(file_extension(FileFraming::Array, true), "jsonld");
        assert_eq!(file_extension(FileFraming::LineDelimited, false), "jsonl");
        assert_eq!(file_extension(FileFraming::LineDelimited, true), "ndjsonld");
    }

    #[test]
    fn line_delimited_framing_without_a_context_writes_a_jsonl_file() {
        let directory = TempDir::new().unwrap();
        let mut writer = framed_writer(&directory, FileFraming::LineDelimited, ContextSource::None);

        writer.write(entity("Sensor", "urn:ngsi-ld:Sensor:1")).unwrap();
        writer.write(entity("Sensor", "urn:ngsi-ld:Sensor:2")).unwrap();
        let stats = writer.finalize(RunOutcome::Committed).unwrap();

        assert_eq!(stats.written, 2);
        let content = fs::read_to_string(directory.path().join("Sensor.jsonl")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["id"], "urn:ngsi-ld:Sensor:1");
    }

    #[test]
    fn line_delimited_framing_with_a_context_writes_an_ndjsonld_file() {
        let directory = TempDir::new().unwrap();
        let context = ContextSource::Static(NgsiLdContext::remote(Url::parse("https://example.com/ctx.jsonld").unwrap()));
        let mut writer = framed_writer(&directory, FileFraming::LineDelimited, context);

        writer.write(entity("Sensor", "urn:ngsi-ld:Sensor:1")).unwrap();
        let stats = writer.finalize(RunOutcome::Committed).unwrap();

        assert_eq!(stats.written, 1);
        let file_body = fs::read_to_string(directory.path().join("Sensor.ndjsonld")).unwrap();
        let line: serde_json::Value = serde_json::from_str(file_body.lines().next().unwrap()).unwrap();
        assert!(line.get("@context").is_some());
    }
}
