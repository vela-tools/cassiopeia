use cassiopeia_mapping::mapping::Mapping;
use smallvec::{SmallVec, smallvec};
use std::sync::Arc;

/// The mappings that govern one payload.
///
/// A [`Record`](crate::record::Record) or [`Fragment`](crate::fragment::Fragment) always carries
/// exactly one; an extracted [`Entity`](crate::entity::Entity) may carry several, one per mapping
/// that contributed to the same entity id (the current-state join of, say, a static geometry mapping
/// and a temporal state mapping). The inline capacity of one keeps the common single-mapping case
/// off the heap.
pub type Mappings = SmallVec<[Arc<Mapping>; 1]>;

/// A wrapper that pairs a data payload with the mapping configuration(s) that produced or will
/// consume it. A [`Mapped`] always carries at least one mapping.
#[derive(Debug, Clone)]
pub struct Mapped<T> {
    /// The data payload (Record, Fragment, or Entity).
    inner: T,
    /// The mapping configuration(s) associated with the data; never empty.
    mappings: Mappings,
}

impl<T> Mapped<T> {
    /// Pairs a data payload with the single mapping that governs it, the common case.
    pub fn new(inner: T, mapping: Arc<Mapping>) -> Mapped<T> {
        Mapped {
            inner,
            mappings: smallvec![mapping],
        }
    }

    /// Pairs a data payload with several mappings, for an entity assembled from more than one
    /// mapping under a shared id. The caller guarantees `mappings` is non-empty.
    pub const fn with_mappings(inner: T, mappings: Mappings) -> Mapped<T> {
        Mapped { inner, mappings }
    }

    /// The wrapped data payload.
    pub const fn inner(&self) -> &T {
        &self.inner
    }

    /// The first mapping governing the payload, for single-mapping consumers.
    pub fn mapping(&self) -> &Arc<Mapping> {
        &self.mappings[0]
    }

    /// Every mapping governing the payload, in first-seen order.
    pub fn mappings(&self) -> &[Arc<Mapping>] {
        &self.mappings
    }

    /// Consumes the wrapper and returns the inner data.
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// Consumes the wrapper and returns the data with its first mapping, for single-mapping
    /// consumers such as fragment resolution.
    pub fn into_parts(self) -> (T, Arc<Mapping>) {
        let mut mappings = self.mappings;
        let mapping = mappings.swap_remove(0);
        (self.inner, mapping)
    }

    /// Consumes the wrapper and returns the data with every mapping governing it.
    pub fn into_mappings(self) -> (T, Mappings) {
        (self.inner, self.mappings)
    }

    /// Transforms the inner data while preserving the mapping configuration(s).
    pub fn map<U, F>(self, f: F) -> Mapped<U>
    where
        F: FnOnce(T) -> U,
    {
        Mapped {
            inner: f(self.inner),
            mappings: self.mappings,
        }
    }
}

impl<T> AsRef<T> for Mapped<T> {
    fn as_ref(&self) -> &T {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use crate::mapped::{Mapped, Mappings};
    use cassiopeia_mapping::{mapping::Mapping, template::runner::TemplateRunner};
    use std::{path::Path, sync::Arc};

    const DOCUMENT: &str = r#"{
        version: "v4",
        dataModel: "AirQualityObserved",
        identity: { entityName: "Station-{{ id }}" },
        attributes: { temperature: { source: "{{ temperature }}" } },
    }"#;

    fn mapping() -> Arc<Mapping> {
        let mut runner = TemplateRunner::new();
        Arc::new(Mapping::from_json5(DOCUMENT, Path::new("test.json5"), &mut runner).expect("valid mapping"))
    }

    #[test]
    fn map_transforms_the_inner_value_and_keeps_the_same_mapping() {
        let mapped = Mapped::new(2_u32, mapping());
        let original = Arc::clone(mapped.mapping());
        let mapped = mapped.map(|value| value * 10);
        assert_eq!(*mapped.inner(), 20);
        assert!(Arc::ptr_eq(&original, mapped.mapping()));
    }

    #[test]
    fn into_parts_and_as_ref_expose_the_wrapped_data() {
        let mapped = Mapped::new("payload".to_string(), mapping());
        assert_eq!(AsRef::<String>::as_ref(&mapped), "payload");
        let (inner, _mapping) = mapped.into_parts();
        assert_eq!(inner, "payload");
    }

    #[test]
    fn with_mappings_carries_every_mapping_and_names_the_first() {
        let mappings = Mappings::from_iter([mapping(), mapping()]);
        let mapped = Mapped::with_mappings(0_u32, mappings);
        assert_eq!(mapped.mappings().len(), 2);
        let (inner, carried) = mapped.into_mappings();
        assert_eq!(inner, 0);
        assert_eq!(carried.len(), 2);
    }
}
