use crate::sensorthings::limit_reached::LimitReached;

/// What collecting a lane's sources met that the run carries on past, for the run to report once
/// collection has ended.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CollectionWarnings {
    /// The collection limits that stopped a read before the server's last page.
    limits_reached: Vec<LimitReached>,
}

impl CollectionWarnings {
    /// The collection limits that stopped a read before the server's last page.
    #[must_use]
    pub fn limits_reached(&self) -> &[LimitReached] {
        &self.limits_reached
    }

    /// Records that a collection limit stopped a read.
    pub fn limit_reached(&mut self, limit: LimitReached) {
        self.limits_reached.push(limit);
    }
}
