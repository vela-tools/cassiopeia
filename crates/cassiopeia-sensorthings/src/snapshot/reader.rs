use crate::snapshot::{
    error::SnapshotError,
    header::SnapshotHeader,
    line::{LineKind, RawLine, SnapshotLine},
    line_number::LineNumber,
};
use std::io::BufRead;

/// Reads a snapshot line by line, so a snapshot of any size is read in the memory of one line.
///
/// The reader yields every line after the header, in file order, and stops at the first line it
/// cannot read, or at the first line of a kind that comes before the kind of a line already read.
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
    /// Among the lines of a kind: the kind of the line read last, or data stream lines right after
    /// the header. A line of this kind or of a later one belongs here.
    Lines(LineKind),
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
            section: Section::Lines(LineKind::Datastream),
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
        if let Section::Lines(after) = self.section {
            let kind = read.kind();
            if kind < after {
                return Err(SnapshotError::OrderViolation { line, kind, after });
            }
            self.section = Section::Lines(kind);
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
            line::{DatastreamLine, HistoricalLocationLine, LineKind, ObservationLine, SnapshotLine, ThingLine},
            reader::SnapshotReader,
            writer::SnapshotWriter,
        },
        stream_id::StreamId,
        stream_kind::StreamKind,
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
        let datastreams = [
            DatastreamLine::new(StreamKind::Datastream, object(json!({"@iot.id": 5, "name": "pm10"}))),
            DatastreamLine::new(StreamKind::Datastream, object(json!({"@iot.id": 9, "name": "no2"}))),
            DatastreamLine::new(StreamKind::MultiDatastream, object(json!({"@iot.id": 5, "name": "ws-7"}))),
        ];
        let observations = [
            (object(json!({"@iot.id": 1, "result": 1.5})), StreamId::Datastream(EntityId::Integer(5))),
            (object(json!({"@iot.id": 2, "result": 2.5})), StreamId::Datastream(EntityId::Integer(5))),
            (object(json!({"@iot.id": 3, "result": 3.5})), StreamId::Datastream(EntityId::Integer(9))),
            (
                object(json!({"@iot.id": 4, "result": [4.2, 270]})),
                StreamId::MultiDatastream(EntityId::Integer(5)),
            ),
        ];

        let mut writer = SnapshotWriter::start(Vec::new(), &header).unwrap();
        for datastream in &datastreams {
            writer.datastream(datastream).unwrap();
        }
        let historical_locations = [
            (object(json!({"@iot.id": 88, "time": "2026-11-12T09:04:00Z"})), EntityId::Integer(12512)),
            (object(json!({"@iot.id": 89, "time": "2026-11-12T09:34:00Z"})), EntityId::Integer(12512)),
        ];
        let mut writer = writer.into_observations();
        for (observation, datastream) in &observations {
            writer.observation(&ObservationLine::new(observation.clone(), datastream.clone())).unwrap();
        }
        let mut writer = writer.into_historical_locations();
        for (historical_location, thing) in &historical_locations {
            writer
                .historical_location(&HistoricalLocationLine::new(historical_location.clone(), thing.clone()))
                .unwrap();
        }
        let written = String::from_utf8(writer.finish().unwrap()).unwrap();

        let expected: Vec<SnapshotLine> = datastreams
            .into_iter()
            .map(SnapshotLine::Datastream)
            .chain(
                observations
                    .into_iter()
                    .map(|(observation, datastream)| SnapshotLine::Observation(ObservationLine::new(observation, datastream))),
            )
            .chain(
                historical_locations
                    .into_iter()
                    .map(|(historical_location, thing)| SnapshotLine::HistoricalLocation(HistoricalLocationLine::new(historical_location, thing))),
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
        let multi = r#"{"multiDatastream":{"@iot.id":4}}"#;
        let observation = r#"{"observation":{"@iot.id":1},"datastream":5}"#;

        assert!(matches!(read_all(""), Err(SnapshotError::MissingHeader)));
        assert!(matches!(read_all("{\"sensorthingsSnapshot\":2}\n"), Err(SnapshotError::UnsupportedVersion { found }) if found == json!(2)));
        assert!(matches!(
            read_all(&format!("{HEADER}\n{datastream}\nnot json\n")),
            Err(SnapshotError::Malformed { line, .. }) if line.get() == 3
        ));
        assert!(matches!(
            read_all(&format!("{HEADER}\n{datastream}\n{observation}\n{observation}\n{datastream}\n")),
            Err(SnapshotError::OrderViolation { line, kind: LineKind::Datastream, after: LineKind::Observation }) if line.get() == 5
        ));
        assert!(matches!(
            read_all(&format!("{HEADER}\n{datastream}\n{observation}\n{multi}\n")),
            Err(SnapshotError::OrderViolation { line, .. }) if line.get() == 4
        ));
        assert!(matches!(
            read_all(&format!("{HEADER}\n{{\"thing\":12512}}\n")),
            Err(SnapshotError::Malformed { line, .. }) if line.get() == 2
        ));
    }

    #[test]
    fn historical_location_lines_come_last() {
        let datastream = r#"{"datastream":{"@iot.id":5}}"#;
        let observation = r#"{"observation":{"@iot.id":1},"datastream":5}"#;
        let historical = r#"{"historicalLocation":{"@iot.id":88},"thing":12512}"#;

        let (_, lines) = read_all(&format!("{HEADER}\n{datastream}\n{historical}\n{historical}\n")).unwrap();
        assert_eq!(
            lines.iter().map(SnapshotLine::kind).collect::<Vec<_>>(),
            [LineKind::Datastream, LineKind::HistoricalLocation, LineKind::HistoricalLocation]
        );
        assert!(matches!(
            read_all(&format!("{HEADER}\n{datastream}\n{observation}\n{historical}\n{datastream}\n")),
            Err(SnapshotError::OrderViolation { line, kind: LineKind::Datastream, after: LineKind::HistoricalLocation }) if line.get() == 5
        ));
        assert!(matches!(
            read_all(&format!("{HEADER}\n{datastream}\n{historical}\n{observation}\n")),
            Err(SnapshotError::OrderViolation { line, kind: LineKind::Observation, after: LineKind::HistoricalLocation }) if line.get() == 4
        ));
    }

    #[test]
    fn thing_lines_follow_the_data_stream_lines() {
        let datastream = r#"{"datastream":{"@iot.id":5}}"#;
        let thing = r#"{"thing":{"@iot.id":12520,"name":"ILM_I75"}}"#;

        let (_, lines) = read_all(&format!("{HEADER}\n{datastream}\n{thing}\n{thing}\n")).unwrap();
        assert_eq!(
            lines.iter().map(SnapshotLine::kind).collect::<Vec<_>>(),
            [LineKind::Datastream, LineKind::Thing, LineKind::Thing]
        );
        assert_eq!(
            lines[1],
            SnapshotLine::Thing(ThingLine::new(object(json!({"@iot.id": 12520, "name": "ILM_I75"}))))
        );
        let (_, without) = read_all(&format!("{HEADER}\n{thing}\n")).unwrap();
        assert_eq!(without.len(), 1);
        assert!(matches!(
            read_all(&format!("{HEADER}\n{datastream}\n{thing}\n{datastream}\n")),
            Err(SnapshotError::OrderViolation { line, kind: LineKind::Datastream, after: LineKind::Thing }) if line.get() == 4
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
