use crate::error::ExpanderError;
use ahash::RandomState;
use cassiopeia_common::collection::CollectionName;
use cassiopeia_mapping::mapping::Mapping;
use std::{collections::HashMap, sync::Arc};

/// The mappings a collections router selects from, keyed by the verbatim source collection label.
///
/// The labels come straight out of the ingested source, so the map hashes with `ahash` rather than
/// the standard library's `SipHash`: it resists a hostile key distribution at a fraction of the cost,
/// and every record probes it.
pub type CollectionRoutes = HashMap<CollectionName, Arc<Mapping>, RandomState>;

/// Selects the compiled mapping that governs a record, given its source collection label.
///
/// A [`Single`](MappingRouter::Single) router applies one mapping to every record and ignores the
/// collection label (this is both the non-collection formats and the KML "merge every folder into
/// one entity" mode). A [`Collections`](MappingRouter::Collections) router routes each record by the
/// verbatim label the ingestor tagged it with, so one multi-collection source yields several types.
pub enum MappingRouter {
    /// One mapping for every record, regardless of collection.
    Single(Arc<Mapping>),
    /// One mapping per source collection label.
    Collections(CollectionRoutes),
}

impl MappingRouter {
    /// Selects the mapping for a record's collection label.
    ///
    /// # Errors
    /// Returns [`ExpanderError::UnmatchedCollection`] when a `Collections` router has no mapping for
    /// the label, and [`ExpanderError::CollectionMissing`] when a `Collections` router is handed a
    /// record with no label at all. A `Single` router never fails.
    pub fn select(&self, collection: Option<&CollectionName>) -> Result<&Arc<Mapping>, ExpanderError> {
        match self {
            MappingRouter::Single(mapping) => Ok(mapping),
            MappingRouter::Collections(mappings) => match collection {
                Some(name) => mappings.get(name).ok_or_else(|| ExpanderError::UnmatchedCollection(name.clone())),
                None => Err(ExpanderError::CollectionMissing),
            },
        }
    }

    /// The distinct mappings this router can select, for stages that inspect every mapping a lane
    /// carries (such as `@context` resolution).
    #[must_use]
    pub fn mappings(&self) -> Vec<&Arc<Mapping>> {
        match self {
            MappingRouter::Single(mapping) => vec![mapping],
            MappingRouter::Collections(mappings) => mappings.values().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::ExpanderError,
        router::{CollectionRoutes, MappingRouter},
    };
    use cassiopeia_common::collection::CollectionName;
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use std::{path::Path, sync::Arc};

    fn mapping(data_model: &str) -> Arc<Mapping> {
        let document = format!(
            r#"{{ version: "v4", dataModel: "{data_model}", identity: {{ entityName: "E-{{{{ id }}}}" }}, attributes: {{ v: {{ source: "{{{{ v }}}}" }} }} }}"#
        );
        let mut runner = TemplateRunner::new();
        Arc::new(Mapping::from_json5(&document, Path::new("test.json5"), &mut runner).unwrap())
    }

    #[test]
    fn a_single_router_ignores_the_collection_label() {
        let mapping = mapping("Sensor");
        let router = MappingRouter::Single(Arc::clone(&mapping));

        assert!(Arc::ptr_eq(router.select(None).unwrap(), &mapping));
        assert!(Arc::ptr_eq(router.select(Some(&CollectionName::from("Anything"))).unwrap(), &mapping));
    }

    #[test]
    fn a_collections_router_routes_by_the_verbatim_label() {
        let camera = mapping("Camera");
        let sensor = mapping("Sensor");
        let mut mappings = CollectionRoutes::default();
        mappings.insert(CollectionName::from("Camera"), Arc::clone(&camera));
        mappings.insert(CollectionName::from("Flowcount"), Arc::clone(&sensor));
        let router = MappingRouter::Collections(mappings);

        assert!(Arc::ptr_eq(router.select(Some(&CollectionName::from("Camera"))).unwrap(), &camera));
        assert!(Arc::ptr_eq(router.select(Some(&CollectionName::from("Flowcount"))).unwrap(), &sensor));
    }

    #[test]
    fn an_unmatched_label_is_an_error_carrying_the_label() {
        let router = MappingRouter::Collections(CollectionRoutes::default());
        match router.select(Some(&CollectionName::from("Missing"))) {
            Err(ExpanderError::UnmatchedCollection(name)) => assert_eq!(name, CollectionName::from("Missing")),
            other => panic!("expected UnmatchedCollection, got {other:?}"),
        }
    }

    #[test]
    fn a_record_without_a_label_under_collections_is_an_error() {
        let router = MappingRouter::Collections(CollectionRoutes::default());
        assert!(matches!(router.select(None), Err(ExpanderError::CollectionMissing)));
    }

    #[test]
    fn two_labels_can_share_one_mapping_arc() {
        let shared = mapping("Camera");
        let mut mappings = CollectionRoutes::default();
        mappings.insert(CollectionName::from("Camera"), Arc::clone(&shared));
        mappings.insert(CollectionName::from("Camera Area"), Arc::clone(&shared));
        let router = MappingRouter::Collections(mappings);

        let a = router.select(Some(&CollectionName::from("Camera"))).unwrap();
        let b = router.select(Some(&CollectionName::from("Camera Area"))).unwrap();
        assert!(Arc::ptr_eq(a, b));
    }
}
