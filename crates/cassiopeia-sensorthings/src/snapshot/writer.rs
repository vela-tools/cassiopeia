use crate::snapshot::{
    header::SnapshotHeader,
    line::{DatastreamLine, ObservationLine},
};
use serde::Serialize;
use std::io::{self, Write};

/// Writes the header and the datastream lines of a snapshot.
///
/// Every datastream line comes before the first observation line, so the writer hands over to an
/// [`ObservationWriter`] once the datastreams are written, and a snapshot it writes is always in the
/// order a reader accepts. It writes only what it is given, and a header holds no credential, so a
/// snapshot never holds a request header or a credential.
pub struct SnapshotWriter<W> {
    /// Where the snapshot is written.
    target: W,
}

/// Writes the observation lines of a snapshot, after its datastream lines.
pub struct ObservationWriter<W> {
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

    /// Writes one datastream line.
    ///
    /// # Errors
    ///
    /// Returns the [`io::Error`] writing to the target failed with.
    pub fn datastream(&mut self, line: &DatastreamLine) -> io::Result<()> {
        write_line(&mut self.target, line)
    }

    /// Ends the datastream lines, so observation lines can follow.
    #[must_use]
    pub fn into_observations(self) -> ObservationWriter<W> {
        ObservationWriter { target: self.target }
    }

    /// Ends a snapshot without observation lines, as a latest read writes it, and returns its target.
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
            line::{DatastreamLine, ObservationLine},
            writer::SnapshotWriter,
        },
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
        writer.datastream(&DatastreamLine::new(object(json!({"@iot.id": 5})))).unwrap();

        let written = String::from_utf8(writer.finish().unwrap()).unwrap();

        assert_eq!(
            written,
            "{\"sensorthingsSnapshot\":1,\"serviceRoot\":\"https://h.example/v1.1\",\"version\":\"v1.1\",\"read\":\"latest\",\"collectedAt\":\"2026-11-12T10:00:00Z\"}\n{\"datastream\":{\"@iot.id\":5}}\n"
        );
    }

    #[test]
    fn observation_lines_follow_the_datastream_lines() {
        let mut writer = SnapshotWriter::start(Vec::new(), &header(ReadKind::Range)).unwrap().into_observations();
        writer
            .observation(&ObservationLine::new(object(json!({"@iot.id": 1})), EntityId::Integer(5)))
            .unwrap();

        let written = String::from_utf8(writer.finish().unwrap()).unwrap();

        assert!(written.ends_with("\n{\"observation\":{\"@iot.id\":1},\"datastream\":5}\n"), "{written}");
    }
}
