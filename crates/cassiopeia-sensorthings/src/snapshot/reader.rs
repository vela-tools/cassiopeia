use crate::snapshot::{
    error::SnapshotError,
    header::SnapshotHeader,
    line::{RawLine, SnapshotLine},
    line_number::LineNumber,
};
use std::io::BufRead;

/// Reads a snapshot line by line, so a snapshot of any size is read in the memory of one line.
///
/// The reader yields every line after the header, in file order, and stops at the first line it
/// cannot read.
pub struct SnapshotReader<R> {
    /// The snapshot's text.
    source: R,
    /// The number of the line read last.
    line: LineNumber,
    /// Which part of the file the reader is in.
    section: Section,
    /// The text of the line being read, kept so each line reuses one buffer.
    text: String,
}

/// The part of a snapshot a reader is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    /// Between the header and the first observation line, where datastream lines belong.
    Datastreams,
    /// Past the first observation line, where only observation lines belong.
    Observations,
    /// Past the last line, or past a line that could not be read.
    Ended,
}

impl<R: BufRead> SnapshotReader<R> {
    /// Reads the header of the snapshot `source` holds and returns it with a reader of the lines
    /// that follow it.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::MissingHeader`] for an empty snapshot or one whose first line is no
    /// header, [`SnapshotError::UnsupportedVersion`] and [`SnapshotError::Malformed`] for a header
    /// Cassiopeia cannot read, and [`SnapshotError::Read`] when the text cannot be read.
    pub fn open(mut source: R) -> Result<(SnapshotHeader, SnapshotReader<R>), SnapshotError> {
        let mut text = String::new();
        if source.read_line(&mut text).map_err(SnapshotError::Read)? == 0 {
            return Err(SnapshotError::MissingHeader);
        }
        let header = SnapshotHeader::from_line(&text)?;
        let reader = SnapshotReader {
            source,
            line: LineNumber::FIRST,
            section: Section::Datastreams,
            text,
        };
        Ok((header, reader))
    }

    /// The number of the line read last.
    #[must_use]
    pub const fn line(&self) -> LineNumber {
        self.line
    }

    /// Reads the next line, or `None` past the last one.
    fn read_line(&mut self) -> Result<Option<SnapshotLine>, SnapshotError> {
        self.text.clear();
        if self.source.read_line(&mut self.text).map_err(SnapshotError::Read)? == 0 {
            return Ok(None);
        }
        self.line = self.line.next();
        let line = self.line;
        let read = serde_json::from_str::<RawLine>(&self.text)
            .and_then(SnapshotLine::try_from)
            .map_err(|source| SnapshotError::Malformed { line, source })?;
        match (&read, self.section) {
            (SnapshotLine::Datastream(_), Section::Observations) => return Err(SnapshotError::OrderViolation { line }),
            (SnapshotLine::Observation(_), Section::Datastreams) => self.section = Section::Observations,
            (SnapshotLine::Datastream(_), Section::Datastreams | Section::Ended) | (SnapshotLine::Observation(_), Section::Observations | Section::Ended) => {}
        }
        Ok(Some(read))
    }
}

impl<R: BufRead> Iterator for SnapshotReader<R> {
    type Item = Result<SnapshotLine, SnapshotError>;

    fn next(&mut self) -> Option<Result<SnapshotLine, SnapshotError>> {
        if self.section == Section::Ended {
            return None;
        }
        let read = self.read_line();
        if !matches!(read, Ok(Some(_))) {
            self.section = Section::Ended;
        }
        read.transpose()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_id::EntityId,
        read_kind::ReadKind,
        service_root::ServiceRoot,
        snapshot::{
            error::SnapshotError,
            header::SnapshotHeader,
            line::{DatastreamLine, ObservationLine, SnapshotLine},
            reader::SnapshotReader,
            writer::SnapshotWriter,
        },
    };
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};
    use std::io::Cursor;
    use url::Url;

    const HEADER: &str =
        r#"{"sensorthingsSnapshot":1,"serviceRoot":"https://h.example/v1.1","version":"v1.1","read":"range","collectedAt":"2026-11-12T10:00:00Z"}"#;

    fn object(value: Value) -> Map<String, Value> {
        let Value::Object(members) = value else {
            panic!("expected an object");
        };
        members
    }

    fn read_all(text: &str) -> Result<(SnapshotHeader, Vec<SnapshotLine>), SnapshotError> {
        let (header, reader) = SnapshotReader::open(Cursor::new(text.as_bytes()))?;
        Ok((header, reader.collect::<Result<Vec<_>, _>>()?))
    }

    #[test]
    fn a_snapshot_round_trips_in_order() {
        let header = SnapshotHeader::new(
            ServiceRoot::try_from(Url::parse("https://h.example/v1.1").unwrap()).unwrap(),
            ReadKind::Range,
            Utc.with_ymd_and_hms(2026, 11, 12, 10, 0, 0).unwrap(),
        );
        let datastreams = [object(json!({"@iot.id": 5, "name": "pm10"})), object(json!({"@iot.id": 9, "name": "no2"}))];
        let observations = [
            (object(json!({"@iot.id": 1, "result": 1.5})), EntityId::Integer(5)),
            (object(json!({"@iot.id": 2, "result": 2.5})), EntityId::Integer(5)),
            (object(json!({"@iot.id": 3, "result": 3.5})), EntityId::Integer(9)),
        ];

        let mut writer = SnapshotWriter::start(Vec::new(), &header).unwrap();
        for datastream in &datastreams {
            writer.datastream(&DatastreamLine::new(datastream.clone())).unwrap();
        }
        let mut writer = writer.into_observations();
        for (observation, datastream) in &observations {
            writer.observation(&ObservationLine::new(observation.clone(), datastream.clone())).unwrap();
        }
        let written = String::from_utf8(writer.finish().unwrap()).unwrap();

        let expected: Vec<SnapshotLine> = datastreams
            .into_iter()
            .map(|datastream| SnapshotLine::Datastream(DatastreamLine::new(datastream)))
            .chain(
                observations
                    .into_iter()
                    .map(|(observation, datastream)| SnapshotLine::Observation(ObservationLine::new(observation, datastream))),
            )
            .collect();
        assert_eq!(read_all(&written).unwrap(), (header, expected));
    }

    #[test]
    fn a_header_only_snapshot_has_no_lines() {
        let (header, lines) = read_all(&format!("{HEADER}\n")).unwrap();

        assert_eq!(header.read(), ReadKind::Range);
        assert_eq!(lines, Vec::<SnapshotLine>::new());
    }

    #[test]
    fn a_malformed_snapshot_is_refused() {
        let datastream = r#"{"datastream":{"@iot.id":5}}"#;
        let observation = r#"{"observation":{"@iot.id":1},"datastream":5}"#;

        assert!(matches!(read_all(""), Err(SnapshotError::MissingHeader)));
        assert!(matches!(read_all("{\"sensorthingsSnapshot\":2}\n"), Err(SnapshotError::UnsupportedVersion { found }) if found == json!(2)));
        assert!(matches!(
            read_all(&format!("{HEADER}\n{datastream}\nnot json\n")),
            Err(SnapshotError::Malformed { line, .. }) if line.get() == 3
        ));
        assert!(matches!(
            read_all(&format!("{HEADER}\n{datastream}\n{observation}\n{observation}\n{datastream}\n")),
            Err(SnapshotError::OrderViolation { line }) if line.get() == 5
        ));
        assert!(matches!(
            read_all(&format!("{HEADER}\n{{\"thing\":{{}}}}\n")),
            Err(SnapshotError::Malformed { line, .. }) if line.get() == 2
        ));
    }

    #[test]
    fn reading_stops_at_the_first_line_it_cannot_read() {
        let (_, mut reader) = SnapshotReader::open(Cursor::new(format!("{HEADER}\nnot json\n{{\"datastream\":{{}}}}\n").into_bytes())).unwrap();

        assert!(matches!(reader.next(), Some(Err(SnapshotError::Malformed { .. }))));
        assert!(reader.next().is_none());
    }

    #[test]
    fn the_reader_names_the_line_it_read_last() {
        let (_, mut reader) = SnapshotReader::open(Cursor::new(format!("{HEADER}\n{{\"datastream\":{{}}}}\n").into_bytes())).unwrap();
        assert_eq!(reader.line().get(), 1);

        reader.next().unwrap().unwrap();

        assert_eq!(reader.line().get(), 2);
    }
}
