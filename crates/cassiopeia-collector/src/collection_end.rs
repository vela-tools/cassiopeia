use crate::{collection_warnings::CollectionWarnings, error::CollectorError};

/// How a collection that did not fail ended.
#[derive(Debug, PartialEq, Eq)]
pub enum CollectionEnd {
    /// Every source was collected and handed on, with what collection met that the run carries on
    /// past.
    Collected(CollectionWarnings),
    /// The run's stop ended collection before it was done. A stop is no failure: no source is
    /// handed on after it, and the cycle reports itself stopped.
    ///
    /// A source handed on before the stop is not withheld or recalled. The stop is the run's, so
    /// every later stage reads it too and winds down instead of finishing what it was handed, the
    /// writer aborts rather than commits, and the stopped cycle moves no cursor.
    Stopped,
}

impl CollectionEnd {
    /// How a collection that `error` unwound ended: stopped, when the run's stop unwound it, and
    /// failed with `error` otherwise.
    ///
    /// A read unwinds through [`CollectorError::Interrupted`] once the run is being stopped, and a
    /// stop is no failure of the read.
    ///
    /// # Errors
    ///
    /// Returns `error` unless it is [`CollectorError::Interrupted`].
    pub(crate) fn unwound_by(error: CollectorError) -> Result<CollectionEnd, CollectorError> {
        match error {
            CollectorError::Interrupted { .. } => Ok(CollectionEnd::Stopped),
            failure @ (CollectorError::ClientInit { .. }
            | CollectorError::Http { .. }
            | CollectorError::HttpStatus { .. }
            | CollectorError::Io { .. }
            | CollectorError::ChannelClosed
            | CollectorError::CursorWithheld { .. }
            | CollectorError::LinkLoop { .. }
            | CollectorError::NextLink { .. }
            | CollectorError::PageUnreadable { .. }
            | CollectorError::InvalidFileExtension { .. }) => Err(failure),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{collection_end::CollectionEnd, error::CollectorError};
    use cassiopeia_common::masked_url::MaskedUrl;
    use url::Url;

    #[test]
    fn a_read_the_stop_unwound_ends_as_stopped_and_any_other_failure_stays_a_failure() {
        let url = MaskedUrl::plain(Url::parse("http://127.0.0.1:8765/v1.1").unwrap());
        let interrupted = CollectorError::Interrupted { url: Box::new(url) };

        assert!(matches!(CollectionEnd::unwound_by(interrupted), Ok(CollectionEnd::Stopped)));
        assert!(matches!(
            CollectionEnd::unwound_by(CollectorError::ChannelClosed),
            Err(CollectorError::ChannelClosed)
        ));
    }
}
