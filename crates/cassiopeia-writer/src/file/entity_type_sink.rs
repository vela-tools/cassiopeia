use crate::{
    error::{Result, WriterError},
    file::indent_prefix_writer::IndentPrefixWriter,
};
use cassiopeia_common::{
    error::io::{IoAction, IoError},
    file_framing::FileFraming,
    representation::NgsiLdRepresentation,
    skip_null::NgsiLdSkipNull,
};
use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf, representation::ReprAdapter};
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

/// The write buffer size held open per entity type. Write-buffer capacity is
/// `#entity_types × WRITE_BUFFER_BYTES`.
const WRITE_BUFFER_BYTES: usize = 1024 * 1024;

/// The buffer capacity seeded per entity when no running size estimate is available (the
/// single-entity [`EntityTypeSink::write_entity`] path).
const DEFAULT_ENTITY_BYTES: usize = 256;

/// A streaming per-entity-type sink that appends entities to one file as they arrive.
///
/// Under [`FileFraming::Array`] the on-disk output matches `to_string_pretty` of a
/// `Vec<NgsiLdEntity>` (`sonic-rs` uses the same two-space indent style as `serde_json`) but is
/// produced incrementally. Under [`FileFraming::LineDelimited`] each entity is written compact on its
/// own newline-terminated line, with no surrounding array. Each open file retains only its write
/// buffer; serialized entities are supplied by the writer's bounded preparation batch.
pub struct EntityTypeSink {
    path: PathBuf,
    writer: BufWriter<File>,
    framing: FileFraming,
    first_written: bool,
}

impl EntityTypeSink {
    /// Opens the file backing one entity type under the given framing.
    ///
    /// # Errors
    /// Returns a [`WriterError`] when the file cannot be created.
    pub fn open(output_dir: &Path, entity_type: &NameBuf, extension: &str, framing: FileFraming) -> Result<EntityTypeSink> {
        let path = output_dir.join(format!("{entity_type}.{extension}"));
        let file = File::create(&path).map_err(|source| IoError::FileOperation {
            source,
            path: path.clone(),
            action: IoAction::Create,
        })?;

        Ok(EntityTypeSink {
            path,
            writer: BufWriter::with_capacity(WRITE_BUFFER_BYTES, file),
            framing,
            first_written: false,
        })
    }

    /// Converts one entity into its unframed JSON bytes.
    ///
    /// The entity streams straight into `sonic-rs` through a [`ReprAdapter`], with no intermediate
    /// `serde_json::Value` tree. `capacity_hint` seeds the output buffer from a running size estimate
    /// so a same-typed batch avoids repeated reallocation as each buffer grows.
    ///
    /// This function performs no file access, so callers can run it across a Rayon batch and append
    /// the resulting buffers in order afterward.
    ///
    /// # Errors
    /// Returns a [`WriterError`] when SIMD serialization fails.
    pub(crate) fn serialize_entity(
        entity: &NgsiLdEntity,
        representation: NgsiLdRepresentation,
        skip_null: NgsiLdSkipNull,
        framing: FileFraming,
        capacity_hint: usize,
    ) -> Result<Vec<u8>> {
        let adapter = ReprAdapter::new(entity, representation, skip_null);

        let mut serialized = Vec::with_capacity(capacity_hint);
        match framing {
            FileFraming::Array => sonic_rs::to_writer_pretty(&mut serialized, &adapter),
            FileFraming::LineDelimited => sonic_rs::to_writer(&mut serialized, &adapter),
        }
        .map_err(|source| WriterError::SimdSerialization {
            source,
            entity_type: entity.entity_type.clone(),
        })?;

        Ok(serialized)
    }

    /// Appends one already-serialized entity, adding this sink's framing bytes.
    ///
    /// # Errors
    /// Returns a [`WriterError`] when the serialized bytes cannot be written.
    pub(crate) fn write_serialized(&mut self, serialized: &[u8]) -> Result<()> {
        match self.framing {
            FileFraming::Array => {
                let separator: &[u8] = if self.first_written { b",\n  " } else { b"[\n  " };
                self.writer.write_all(separator).map_err(|source| IoError::FileOperation {
                    source,
                    path: self.path.clone(),
                    action: IoAction::Write,
                })?;

                let mut indented = IndentPrefixWriter {
                    inner: &mut self.writer,
                    indent: b"  ",
                };
                indented.write_all(serialized).map_err(|source| IoError::FileOperation {
                    source,
                    path: self.path.clone(),
                    action: IoAction::Write,
                })?;
            }
            FileFraming::LineDelimited => {
                self.writer.write_all(serialized).map_err(|source| IoError::FileOperation {
                    source,
                    path: self.path.clone(),
                    action: IoAction::Write,
                })?;
                self.writer.write_all(b"\n").map_err(|source| IoError::FileOperation {
                    source,
                    path: self.path.clone(),
                    action: IoAction::Write,
                })?;
            }
        }

        self.first_written = true;
        Ok(())
    }

    /// Appends one entity to the file in the sink's framing.
    ///
    /// # Errors
    /// Returns a [`WriterError`] when the entity cannot be serialized or written to the file.
    pub fn write_entity(&mut self, entity: &NgsiLdEntity, representation: NgsiLdRepresentation, skip_null: NgsiLdSkipNull) -> Result<()> {
        let serialized = Self::serialize_entity(entity, representation, skip_null, self.framing, DEFAULT_ENTITY_BYTES)?;
        self.write_serialized(&serialized)
    }

    /// Closes the file, writing the array's closing bytes under [`FileFraming::Array`], then flushes.
    ///
    /// # Errors
    /// Returns a [`WriterError`] when the closing bytes cannot be written or the file cannot be
    /// flushed.
    pub fn finalize(mut self) -> Result<()> {
        // Line-delimited output is already terminated per line and needs no closing bytes; an empty
        // line-delimited sink stays a zero-byte file. Only the array framing brackets its content.
        if self.framing == FileFraming::Array {
            let closing: &[u8] = if self.first_written { b"\n]" } else { b"[]" };
            self.writer.write_all(closing).map_err(|source| IoError::FileOperation {
                source,
                path: self.path.clone(),
                action: IoAction::Write,
            })?;
        }
        self.writer.flush().map_err(|source| IoError::FileOperation {
            source,
            path: self.path,
            action: IoAction::Flush,
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::file::entity_type_sink::EntityTypeSink;
    use cassiopeia_common::{file_framing::FileFraming, representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
    use cassiopeia_ngsi_ld::entity::{NgsiLdEntity, name::NameBuf};
    use std::fs;
    use tempfile::TempDir;
    use urn_rs::Urn;

    fn entity(id: &str) -> NgsiLdEntity {
        NgsiLdEntity::new(id.parse::<Urn>().unwrap(), NameBuf::new("Sensor").unwrap())
    }

    #[test]
    fn an_empty_array_sink_finalizes_to_an_empty_json_array() {
        let directory = TempDir::new().unwrap();
        let sink = EntityTypeSink::open(directory.path(), &NameBuf::new("Sensor").unwrap(), "json", FileFraming::Array).unwrap();

        sink.finalize().unwrap();

        assert_eq!(fs::read_to_string(directory.path().join("Sensor.json")).unwrap(), "[]");
    }

    #[test]
    fn written_entities_frame_a_json_array() {
        let directory = TempDir::new().unwrap();
        let mut sink = EntityTypeSink::open(directory.path(), &NameBuf::new("Sensor").unwrap(), "json", FileFraming::Array).unwrap();

        sink.write_entity(&entity("urn:ngsi-ld:Sensor:1"), NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip)
            .unwrap();
        sink.write_entity(&entity("urn:ngsi-ld:Sensor:2"), NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip)
            .unwrap();
        sink.finalize().unwrap();

        let content = fs::read_to_string(directory.path().join("Sensor.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let array = parsed.as_array().unwrap();

        assert_eq!(array.len(), 2);
        assert_eq!(array[0]["id"], "urn:ngsi-ld:Sensor:1");
    }

    #[test]
    fn an_empty_line_delimited_sink_finalizes_to_a_zero_byte_file() {
        let directory = TempDir::new().unwrap();
        let sink = EntityTypeSink::open(directory.path(), &NameBuf::new("Sensor").unwrap(), "jsonl", FileFraming::LineDelimited).unwrap();

        sink.finalize().unwrap();

        assert_eq!(fs::read(directory.path().join("Sensor.jsonl")).unwrap().len(), 0);
    }

    #[test]
    fn written_entities_become_newline_terminated_compact_lines() {
        let directory = TempDir::new().unwrap();
        let mut sink = EntityTypeSink::open(directory.path(), &NameBuf::new("Sensor").unwrap(), "jsonl", FileFraming::LineDelimited).unwrap();

        sink.write_entity(&entity("urn:ngsi-ld:Sensor:1"), NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip)
            .unwrap();
        sink.write_entity(&entity("urn:ngsi-ld:Sensor:2"), NgsiLdRepresentation::Normalized, NgsiLdSkipNull::Skip)
            .unwrap();
        sink.finalize().unwrap();

        let content = fs::read_to_string(directory.path().join("Sensor.jsonl")).unwrap();
        let lines: Vec<&str> = content.lines().collect();

        assert_eq!(lines.len(), 2);
        assert!(content.ends_with('\n'));
        for (line, expected_id) in lines.iter().zip(["urn:ngsi-ld:Sensor:1", "urn:ngsi-ld:Sensor:2"]) {
            // A compact line carries no pretty-print indentation.
            assert!(!line.contains("  "));
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(parsed["id"], expected_id);
        }
    }
}

/// Confirms the streaming `sonic-rs` write path and the `serde_json` tree path
/// (`NgsiLdEntity::to_json`) produce the same document, across every representation and
/// null-handling mode.
#[cfg(test)]
mod parity_tests {
    use crate::file::entity_type_sink::EntityTypeSink;
    use cassiopeia_common::{file_framing::FileFraming, representation::NgsiLdRepresentation, skip_null::NgsiLdSkipNull};
    use cassiopeia_geometry::geometry::NgsiLdGeometry;
    use cassiopeia_ngsi_ld::{
        entity::{
            NgsiLdEntity,
            attribute::{NgsiLdAttribute, NgsiLdAttributeWrapper, geo_property::NgsiLdGeoProperty, property::NgsiLdProperty, relationship::NgsiLdRelationship},
            name::NameBuf,
            representation::NgsiLdSerializable,
        },
        value::types::Value,
    };
    use chrono::{TimeZone, Utc};
    use indexmap::IndexMap;
    use serde_json::{Value as JsonValue, json};
    use urn_rs::Urn;

    // A property carrying observed-at, a dataset id, and a nested sub-attribute, exercising the
    // qualifier and nested-attribute paths in both the normalized map and the concise collapse.
    fn temperature() -> NgsiLdAttributeWrapper {
        let mut nested = IndexMap::default();
        nested.insert(
            NameBuf::new("accuracy").unwrap(),
            Box::new(NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty::new(0.95)))),
        );

        let mut property = NgsiLdProperty::new(Value::from(json!(21.5)));
        property.observed_at = Some(Utc.with_ymd_and_hms(2024, 3, 13, 12, 0, 0).unwrap());
        property.dataset_id = Some("urn:ngsi-ld:Dataset:indoor".parse::<Urn>().unwrap());
        property.attributes = nested;
        NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(property))
    }

    // A relationship carrying an object type, exercising the relationship qualifier path.
    fn ref_device() -> NgsiLdAttributeWrapper {
        let mut relationship = NgsiLdRelationship::new("urn:ngsi-ld:Device:1".parse::<Urn>().unwrap());
        relationship.object_type = Some(NameBuf::new("Device").unwrap());
        NgsiLdAttributeWrapper::single(NgsiLdAttribute::Relationship(relationship))
    }

    // Two dataset-distinguished instances, exercising the multi-instance array path.
    fn readings() -> NgsiLdAttributeWrapper {
        let mut first = NgsiLdProperty::new(Value::from(json!(1)));
        first.dataset_id = Some("urn:ngsi-ld:Dataset:a".parse::<Urn>().unwrap());
        let mut second = NgsiLdProperty::new(Value::from(json!(2)));
        second.dataset_id = Some("urn:ngsi-ld:Dataset:b".parse::<Urn>().unwrap());
        NgsiLdAttributeWrapper::Multi(vec![NgsiLdAttribute::Property(first), NgsiLdAttribute::Property(second)])
    }

    fn representative_entity() -> NgsiLdEntity {
        let mut attributes = IndexMap::default();
        attributes.insert(NameBuf::new("temperature").unwrap(), temperature());
        attributes.insert(
            NameBuf::new("status").unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty::new(Value::from(json!("active"))))),
        );
        // A null-valued property, dropped only under skip-null.
        attributes.insert(
            NameBuf::new("spare").unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::Property(NgsiLdProperty::new(Value::Null))),
        );
        attributes.insert(NameBuf::new("refDevice").unwrap(), ref_device());
        attributes.insert(
            NameBuf::new("location").unwrap(),
            NgsiLdAttributeWrapper::single(NgsiLdAttribute::GeoProperty(NgsiLdGeoProperty::new(NgsiLdGeometry::Point {
                coordinates: [1.5, 2.5].into(),
            }))),
        );
        attributes.insert(NameBuf::new("readings").unwrap(), readings());

        NgsiLdEntity {
            id: "urn:ngsi-ld:Sensor:1".parse::<Urn>().unwrap(),
            entity_type: NameBuf::new("Sensor").unwrap(),
            context: None,
            scope: None,
            attributes,
        }
    }

    #[test]
    fn streamed_output_matches_the_serde_json_tree_in_every_mode() {
        let entity = representative_entity();
        let representations = [
            NgsiLdRepresentation::Normalized,
            NgsiLdRepresentation::Concise,
            NgsiLdRepresentation::Simplified,
        ];

        for representation in representations {
            for skip_null in [NgsiLdSkipNull::Skip, NgsiLdSkipNull::Include] {
                for framing in [FileFraming::Array, FileFraming::LineDelimited] {
                    let bytes = EntityTypeSink::serialize_entity(&entity, representation, skip_null, framing, 256).unwrap();
                    let streamed: JsonValue = serde_json::from_slice(&bytes).unwrap();
                    let tree = entity.to_json(representation, skip_null).unwrap();

                    assert_eq!(streamed, tree, "representation={representation:?} skip_null={skip_null:?} framing={framing:?}");
                }
            }
        }
    }
}
