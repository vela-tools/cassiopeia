use cassiopeia_geometry::policy::GeometryPolicy;
use cassiopeia_ngsi_ld::value::omitted_nulls::OmittedNulls;

/// What every reading of one declaration's value shares: the geometry policy a geometry is read
/// under, and the count of the nulls left out of the value built.
///
/// The policy is the declaration's own, or the lossless default when it declares none, settled once
/// before the value is read.
#[derive(Debug)]
pub(crate) struct ReadContext<'a> {
    /// What may be lost bringing a geometry read here to the type its conversion names.
    pub(crate) policy: GeometryPolicy,
    /// Where each null left out of the value is counted.
    pub(crate) omitted: &'a mut OmittedNulls,
}
