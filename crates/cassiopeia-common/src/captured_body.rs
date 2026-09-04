use std::{borrow::Cow, error::Error, str::from_utf8};

/// How completely a response body was captured.
///
/// A capture is bounded so a broker that answers with a multi-megabyte error page cannot be held in
/// memory whole, and the outcome is recorded rather than inferred: a diagnostic that echoes a body
/// has to be able to say the echo is partial.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum BodyCapture {
    /// Every byte the response carried was captured.
    Complete,
    /// The body exceeded the cap and was cut short.
    Truncated {
        /// How many bytes the response carried before the cut.
        total: usize,
    },
    /// The body could not be read off the wire at all.
    Unreadable {
        /// The rendered transport failure, kept as text because the capture outlives the reader.
        reason: Box<str>,
    },
}

/// A response body captured for diagnostics, together with how completely it was captured.
///
/// This lives in the foundation crate because both the broker writer (which reads bodies off the
/// wire) and the collector (which reads them off a schema download) need the identical value, and
/// because a diagnostic carries one as context. It holds bytes only, so the foundation crate gains
/// no HTTP dependency.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CapturedBody {
    /// The captured bytes, never longer than the cap the capture was taken under.
    bytes: Vec<u8>,
    /// How completely those bytes represent the response.
    capture: BodyCapture,
}

impl CapturedBody {
    /// Captures `bytes`, cutting the capture at `cap` bytes when the body is longer.
    ///
    /// The cut lands on a UTF-8 character boundary, so echoing the capture cannot manufacture a
    /// replacement character that reads as corrupt output from the peer. A body that is not UTF-8 at
    /// all is cut at the cap itself: the walk-back only skips continuation bytes, so it never
    /// discards more than three bytes.
    #[must_use]
    pub fn capped(bytes: Vec<u8>, cap: usize) -> CapturedBody {
        let total = bytes.len();
        if total <= cap {
            return CapturedBody {
                bytes,
                capture: BodyCapture::Complete,
            };
        }

        let mut bytes = bytes;
        bytes.truncate(char_boundary(&bytes, cap));
        CapturedBody {
            bytes,
            capture: BodyCapture::Truncated { total },
        }
    }

    /// Records that the body could not be read, naming the transport failure that stopped it.
    #[must_use]
    pub fn unreadable(reason: &dyn Error) -> CapturedBody {
        CapturedBody {
            bytes: Vec::new(),
            capture: BodyCapture::Unreadable {
                reason: reason.to_string().into_boxed_str(),
            },
        }
    }

    /// Re-cuts an already-captured body down to `cap` bytes for display.
    ///
    /// A body is captured generously so it can be deserialized and echoed narrowly so it can be
    /// read, and the original length survives the second cut: a capture that was already truncated
    /// keeps reporting the response's true size, not the size of the first cut.
    #[must_use]
    pub fn echo(&self, cap: usize) -> CapturedBody {
        if self.bytes.len() <= cap {
            return self.clone();
        }

        let total = match &self.capture {
            BodyCapture::Truncated { total } => *total,
            BodyCapture::Complete | BodyCapture::Unreadable { .. } => self.bytes.len(),
        };
        CapturedBody {
            bytes: self.bytes[..char_boundary(&self.bytes, cap)].to_vec(),
            capture: BodyCapture::Truncated { total },
        }
    }

    /// The captured bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// How completely the body was captured.
    #[must_use]
    pub const fn capture(&self) -> &BodyCapture {
        &self.capture
    }

    /// Whether nothing was captured, either because the body was empty or because it was unreadable.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The capture rendered as text, replacing any byte sequence that is not valid UTF-8.
    #[must_use]
    pub fn text(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.bytes)
    }
}

/// The largest index at or below `cap` that does not split a UTF-8 character.
///
/// Walking back over continuation bytes (`10xxxxxx`) never moves more than three positions, because
/// no UTF-8 sequence is longer than four bytes.
fn char_boundary(bytes: &[u8], cap: usize) -> usize {
    let mut end = cap.min(bytes.len());
    while end > 0 && end < bytes.len() && bytes[end] & 0b1100_0000 == 0b1000_0000 {
        end -= 1;
    }
    // A cut that still does not decode (a truncated body that was never valid UTF-8) keeps the cap:
    // the lossy rendering handles it, and discarding good bytes would be worse.
    if from_utf8(&bytes[..end]).is_ok() { end } else { cap.min(bytes.len()) }
}

#[cfg(test)]
mod tests {
    use crate::captured_body::{BodyCapture, CapturedBody};
    use std::io::Error;

    #[test]
    fn a_body_within_the_cap_is_captured_whole() {
        let body = CapturedBody::capped(b"{\"detail\":\"bad\"}".to_vec(), 64);

        assert_eq!(body.capture(), &BodyCapture::Complete);
        assert_eq!(body.text(), "{\"detail\":\"bad\"}");
    }

    #[test]
    fn a_body_past_the_cap_is_cut_and_reports_its_true_length() {
        let body = CapturedBody::capped(vec![b'x'; 100], 10);

        assert_eq!(body.capture(), &BodyCapture::Truncated { total: 100 });
        assert_eq!(body.bytes().len(), 10);
    }

    #[test]
    fn a_cut_lands_on_a_character_boundary() {
        // Each 'é' is two bytes, so a cap of 3 falls inside the second one and must walk back to 2.
        let body = CapturedBody::capped("éé".as_bytes().to_vec(), 3);

        assert_eq!(body.text(), "é");
        assert!(!body.text().contains('\u{fffd}'));
    }

    #[test]
    fn an_unreadable_body_carries_the_reason_and_no_bytes() {
        let body = CapturedBody::unreadable(&Error::other("connection reset"));

        assert!(body.is_empty());
        let BodyCapture::Unreadable { reason } = body.capture() else {
            panic!("expected an unreadable capture");
        };
        assert!(reason.contains("connection reset"));
    }

    #[test]
    fn an_echo_narrows_the_capture_while_keeping_the_original_length() {
        let body = CapturedBody::capped(vec![b'y'; 5000], 4096).echo(16);

        assert_eq!(body.bytes().len(), 16);
        assert_eq!(body.capture(), &BodyCapture::Truncated { total: 5000 });
    }

    #[test]
    fn an_echo_of_a_short_body_changes_nothing() {
        let body = CapturedBody::capped(b"short".to_vec(), 4096);

        assert_eq!(body.echo(16), body);
    }
}
