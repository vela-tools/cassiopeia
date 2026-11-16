use crate::snapshot::{
    header::SnapshotHeader,
    line::{DatastreamLine, HistoricalLocationLine, ObservationLine, ThingLine},
};
use serde::Serialize;
use std::io::{self, Write};

/// Writes the header and the data stream lines of a snapshot.
///
/// Every data stream line, of either kind, comes before the first thing line and the first
/// observation line, so the writer hands over to a [`ThingWriter`] or an [`ObservationWriter`] once
/// the data streams are written, and a snapshot it writes is always in the order a reader accepts. It
/// writes only what it is given, and a header holds no credential, so a snapshot never holds a
/// request header or a credential.
pub struct SnapshotWriter<W> {
    /// Where the snapshot is written.
    target: W,
}

/// Writes the thing lines of a latest snapshot, after its data stream lines.
pub struct ThingWriter<W> {
    /// Where the snapshot is written.
    target: W,
}

/// Writes the observation lines of a snapshot, after its data stream lines.
///
/// Every observation line comes before the first historicalLocation line, so the writer hands over
/// to a [`HistoricalLocationWriter`] once the Observations are written.
pub struct ObservationWriter<W> {
    /// Where the snapshot is written.
    target: W,
}

/// Writes the historicalLocation lines of a snapshot, after its observation lines.
pub struct HistoricalLocationWriter<W> {
    /// Where the snapshot is written.
    target: W,
}

impl<W: Write> SnapshotWriter<W> {
    /// Starts a snapshot in `target` with `header` as its first line.
    ///
    /// # Errors
    ///
    /// Returns the [`io::Error`] writing to `target` failed with.
    pub fn start(mut target: W, header: &SnapshotHeader) -> io::Result<SnapshotWriter<W>> {
        write_line(&mut target, header)?;
        Ok(SnapshotWriter { target })
    }

    /// Writes one data stream line.
    ///
    /// # Errors
    ///
    /// Returns the [`io::Error`] writing to the target failed with.
    pub fn datastream(&mut self, line: &DatastreamLine) -> io::Result<()> {
        write_line(&mut self.target, line)
    }

    /// Ends the data stream lines, so observation lines can follow.
    #[must_use]
    pub fn into_observations(self) -> ObservationWriter<W> {
        ObservationWriter { target: self.target }
    }

    /// Ends the data stream lines of a latest snapshot, so thing lines can follow.
    #[must_use]
    pub fn into_things(self) -> ThingWriter<W> {
        ThingWriter { target: self.target }
    }

    /// Ends a snapshot of its header and data stream lines alone, as a range read whose range holds
    /// no reading writes it, and returns its target.
    ///
    /// # Errors
    ///
    /// Returns the [`io::Error`] flushing the target failed with.
    pub fn finish(self) -> io::Result<W> {
        finish(self.target)
    }
}

impl<W: Write> ThingWriter<W> {
    /// Writes one thing line.
    ///
    /// # Errors
    ///
    /// Returns the [`io::Error`] writing to the target failed with.
    pub fn thing(&mut self, line: &ThingLine) -> io::Result<()> {
        write_line(&mut self.target, line)
    }

    /// Ends a latest snapshot, which holds no observation line, and returns its target.
    ///
    /// # Errors
    ///
    /// Returns the [`io::Error`] flushing the target failed with.
    pub fn finish(self) -> io::Result<W> {
        finish(self.target)
    }
}

impl<W: Write> ObservationWriter<W> {
    /// Writes one observation line.
    ///
    /// # Errors
    ///
    /// Returns the [`io::Error`] writing to the target failed with.
    pub fn observation(&mut self, line: &ObservationLine) -> io::Result<()> {
        write_line(&mut self.target, line)
    }

    /// Ends the observation lines, so historicalLocation lines can follow.
    #[must_use]
    pub fn into_historical_locations(self) -> HistoricalLocationWriter<W> {
        HistoricalLocationWriter { target: self.target }
    }

    /// Ends the snapshot and returns its target.
    ///
    /// # Errors
    ///
    /// Returns the [`io::Error`] flushing the target failed with.
    pub fn finish(self) -> io::Result<W> {
        finish(self.target)
    }
}

impl<W: Write> HistoricalLocationWriter<W> {
    /// Writes one historicalLocation line.
    ///
    /// # Errors
    ///
    /// Returns the [`io::Error`] writing to the target failed with.
    pub fn historical_location(&mut self, line: &HistoricalLocationLine) -> io::Result<()> {
        write_line(&mut self.target, line)
    }

    /// Ends the snapshot and returns its target.
    ///
    /// # Errors
    ///
    /// Returns the [`io::Error`] flushing the target failed with.
    pub fn finish(self) -> io::Result<W> {
        finish(self.target)
    }
}

/// Writes `line` as one JSON object followed by `\n`.
fn write_line(target: &mut impl Write, line: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer(&mut *target, line)?;
    target.write_all(b"\n")
}

/// Flushes `target` and hands it back.
fn finish<W: Write>(mut target: W) -> io::Result<W> {
    target.flush()?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_id::EntityId,
        read_kind::ReadKind,
        service_root::ServiceRoot,
        snapshot::{
            header::SnapshotHeader,
            line::{DatastreamLine, HistoricalLocationLine, ObservationLine, ThingLine},
            writer::SnapshotWriter,
        },
        stream_id::StreamId,
        stream_kind::StreamKind,
    };
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Map, Value, json};
    use url::Url;

    fn header(read: ReadKind) -> SnapshotHeader {
        SnapshotHeader::new(
            ServiceRoot::try_from(Url::parse("https://h.example/v1.1").unwrap()).unwrap(),
            read,
            Utc.with_ymd_and_hms(2026, 11, 12, 10, 0, 0).unwrap(),
        )
    }

    fn object(value: Value) -> Map<String, Value> {
        let Value::Object(members) = value else {
            panic!("expected an object");
        };
        members
    }

    #[test]
    fn a_latest_snapshot_is_its_header_and_its_datastream_lines() {
        let mut writer = SnapshotWriter::start(Vec::new(), &header(ReadKind::Latest)).unwrap();
        writer
            .datastream(&DatastreamLine::new(StreamKind::Datastream, object(json!({"@iot.id": 5}))))
            .unwrap();

        let written = String::from_utf8(writer.into_things().finish().unwrap()).unwrap();

        assert_eq!(
            written,
            "{\"sensorthingsSnapshot\":1,\"serviceRoot\":\"https://h.example/v1.1\",\"version\":\"v1.1\",\"read\":\"latest\",\"collectedAt\":\"2026-11-12T10:00:00Z\"}\n{\"datastream\":{\"@iot.id\":5}}\n"
        );
    }

    #[test]
    fn thing_lines_follow_the_datastream_lines_of_a_latest_snapshot() {
        let mut writer = SnapshotWriter::start(Vec::new(), &header(ReadKind::Latest)).unwrap();
        writer
            .datastream(&DatastreamLine::new(StreamKind::Datastream, object(json!({"@iot.id": 5}))))
            .unwrap();
        let mut writer = writer.into_things();
        writer.thing(&ThingLine::new(object(json!({"@iot.id": 12520, "name": "ILM_I75"})))).unwrap();

        let written = String::from_utf8(writer.finish().unwrap()).unwrap();

        assert!(
            written.ends_with("\n{\"datastream\":{\"@iot.id\":5}}\n{\"thing\":{\"@iot.id\":12520,\"name\":\"ILM_I75\"}}\n"),
            "{written}"
        );
    }

    #[test]
    fn observation_lines_follow_the_datastream_lines() {
        let mut writer = SnapshotWriter::start(Vec::new(), &header(ReadKind::Range)).unwrap().into_observations();
        writer
            .observation(&ObservationLine::new(object(json!({"@iot.id": 1})), StreamId::Datastream(EntityId::Integer(5))))
            .unwrap();

        let written = String::from_utf8(writer.finish().unwrap()).unwrap();

        assert!(written.ends_with("\n{\"observation\":{\"@iot.id\":1},\"datastream\":5}\n"), "{written}");
    }

    #[test]
    fn historical_location_lines_follow_the_observation_lines() {
        let mut writer = SnapshotWriter::start(Vec::new(), &header(ReadKind::Range))
            .unwrap()
            .into_observations()
            .into_historical_locations();
        writer
            .historical_location(&HistoricalLocationLine::new(object(json!({"@iot.id": 88})), EntityId::Integer(12512)))
            .unwrap();

        let written = String::from_utf8(writer.finish().unwrap()).unwrap();

        assert!(written.ends_with("\n{\"historicalLocation\":{\"@iot.id\":88},\"thing\":12512}\n"), "{written}");
    }
}
