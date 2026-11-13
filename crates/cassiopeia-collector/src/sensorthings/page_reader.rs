use crate::{downloader::Downloader, error::CollectorError, sensorthings::request_pacing::RequestPacing};
use cassiopeia_common::{http_retry::backoff::BackoffSleep, masked_url::MaskedUrl};
use cassiopeia_sensorthings::{paging::page::Page, service_root::ServiceRoot, settings::request_delay::RequestDelay};
use std::sync::atomic::{AtomicBool, Ordering};
use url::Url;

/// Reads the pages of one cycle from a SensorThings server, one request at a time and spaced as the
/// input's settings ask.
pub(crate) struct PageReader {
    /// The server's service root, which a failure names.
    root: ServiceRoot,
    /// The downloader every page is fetched through, with the input's headers.
    downloader: Downloader,
    /// The spacing between the cycle's requests.
    pacing: RequestPacing,
    /// The process-wide shutdown flag, set while the run is being stopped.
    shutdown: &'static AtomicBool,
}

impl PageReader {
    /// A reader of `root`'s pages through `downloader`, waiting `delay` between requests, that stops
    /// once `shutdown` is set.
    pub(crate) const fn new(root: ServiceRoot, downloader: Downloader, delay: Option<RequestDelay>, shutdown: &'static AtomicBool) -> PageReader {
        PageReader {
            root,
            downloader,
            pacing: RequestPacing::new(delay),
            shutdown,
        }
    }

    /// The service root the pages are read from.
    pub(crate) const fn root(&self) -> &ServiceRoot {
        &self.root
    }

    /// Fails with [`CollectorError::Interrupted`] once the run is being stopped.
    pub(crate) fn ensure_running(&self) -> Result<(), CollectorError> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(self.interrupted());
        }
        Ok(())
    }

    /// Requests the page at `url`, once its turn in the cycle has come, and reads its body.
    ///
    /// # Errors
    ///
    /// Returns [`CollectorError::Interrupted`] when the run is stopped during the wait, every error
    /// [`Downloader::fetch`] returns, which name `url`, and [`CollectorError::PageUnreadable`] for a
    /// body that is no page of a collection.
    pub(crate) fn read(&mut self, url: &Url) -> Result<Page, CollectorError> {
        if self.pacing.wait_turn(self.shutdown) == BackoffSleep::ShutdownRequested {
            return Err(self.interrupted());
        }
        // The downloader names the URL in a failure, while the traversal keeps the page's URL to
        // resolve its next link against. A page URL holds no secret: the service root reads none
        // from the environment, and the server wrote the rest.
        let fetched = self.downloader.fetch(&MaskedUrl::plain(url.clone()))?;
        Page::from_body(fetched.bytes()).map_err(|source| CollectorError::PageUnreadable {
            // The error owns the URL, while the traversal keeps the one it asked for.
            url: Box::new(url.clone()),
            source,
        })
    }

    /// The failure of a read the run's stop ended.
    fn interrupted(&self) -> CollectorError {
        CollectorError::Interrupted {
            // The error names the root, while the reader keeps its own.
            root: Box::new(self.root.clone()),
        }
    }
}
