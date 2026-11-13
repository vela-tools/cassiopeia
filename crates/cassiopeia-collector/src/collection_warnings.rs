use crate::sensorthings::{datastreams_missing::DatastreamsMissing, limit_reached::LimitReached, range_empty::RangeEmpty};

/// What collecting a lane's sources met that the run carries on past, for the run to report once
/// collection has ended.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CollectionWarnings {
    /// The collection limits that stopped a read, one per server and limit.
    limits_reached: Vec<LimitReached>,
    /// The range reads whose range held no reading.
    empty_ranges: Vec<RangeEmpty>,
    /// The incremental reads whose server no longer lists data streams their cursor keeps a
    /// position for.
    missing_datastreams: Vec<DatastreamsMissing>,
}

impl CollectionWarnings {
    /// The collection limits that stopped a read, one per server and limit.
    #[must_use]
    pub fn limits_reached(&self) -> &[LimitReached] {
        &self.limits_reached
    }

    /// The range reads whose range held no reading.
    #[must_use]
    pub fn empty_ranges(&self) -> &[RangeEmpty] {
        &self.empty_ranges
    }

    /// The incremental reads whose server no longer lists data streams their cursor keeps a
    /// position for.
    #[must_use]
    pub fn missing_datastreams(&self) -> &[DatastreamsMissing] {
        &self.missing_datastreams
    }

    /// Records that a collection limit stopped a read, unless the same limit already stopped a read
    /// of the same server: a range read follows each data stream on its own, and one limit that stops
    /// several of them is one thing to report.
    pub fn limit_reached(&mut self, limit: LimitReached) {
        if !self
            .limits_reached
            .iter()
            .any(|reached| reached.root() == limit.root() && reached.limit() == limit.limit())
        {
            self.limits_reached.push(limit);
        }
    }

    /// Records that a range read's range held no reading.
    pub fn range_empty(&mut self, range: RangeEmpty) {
        self.empty_ranges.push(range);
    }

    /// Records that an incremental read's server no longer lists data streams its cursor keeps a
    /// position for.
    pub fn datastreams_missing(&mut self, missing: DatastreamsMissing) {
        self.missing_datastreams.push(missing);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        collection_warnings::CollectionWarnings,
        sensorthings::limit_reached::{CollectLimit, LimitReached},
    };
    use cassiopeia_sensorthings::{
        service_root::ServiceRoot,
        settings::collect_limits::{ObservationLimit, PageLimit},
    };
    use pretty_assertions::assert_eq;
    use url::Url;

    fn reached(root: &str, limit: CollectLimit, next: &str) -> LimitReached {
        LimitReached::new(ServiceRoot::try_from(Url::parse(root).unwrap()).unwrap(), limit, Url::parse(next).unwrap())
    }

    #[test]
    fn a_limit_that_stops_several_reads_of_one_server_is_recorded_once() {
        let pages = CollectLimit::Pages(PageLimit::try_from(1).unwrap());
        let observations = CollectLimit::Observations(ObservationLimit::try_from(5).unwrap());
        let mut warnings = CollectionWarnings::default();

        warnings.limit_reached(reached(
            "https://h.example/v1.1",
            pages,
            "https://h.example/v1.1/Datastreams(1)/Observations?$skip=1",
        ));
        warnings.limit_reached(reached(
            "https://h.example/v1.1",
            pages,
            "https://h.example/v1.1/Datastreams(2)/Observations?$skip=1",
        ));
        warnings.limit_reached(reached(
            "https://h.example/v1.1",
            observations,
            "https://h.example/v1.1/Datastreams(3)/Observations",
        ));
        warnings.limit_reached(reached("https://other.example/v1.1", pages, "https://other.example/v1.1/Datastreams?$skip=1"));

        let recorded: Vec<(&str, CollectLimit, &str)> = warnings
            .limits_reached()
            .iter()
            .map(|limit| (limit.root().url().as_str(), limit.limit(), limit.next().as_str()))
            .collect();
        assert_eq!(
            recorded,
            [
                ("https://h.example/v1.1", pages, "https://h.example/v1.1/Datastreams(1)/Observations?$skip=1"),
                ("https://h.example/v1.1", observations, "https://h.example/v1.1/Datastreams(3)/Observations"),
                ("https://other.example/v1.1", pages, "https://other.example/v1.1/Datastreams?$skip=1"),
            ]
        );
    }
}
