//! Temporal aggregation stage for the Cassiopeia pipeline.
//!
//! The aggregator folds the many single-instance observations that share one entity id into a single
//! NGSI-LD `EntityTemporal` (ETSI GS CIM 009 v1.9.1, clause 5.2.20), whose temporal attributes become
//! time-ordered instance arrays. [`Aggregator`](aggregator::Aggregator) is the streaming capability;
//! [`TemporalAggregator`](temporal_aggregator::TemporalAggregator) is the standard implementation,
//! driving [`TemporalAggregate`](cassiopeia_ngsi_ld::entity::temporal_aggregate::TemporalAggregate).

pub mod aggregator;
pub mod temporal_aggregator;
