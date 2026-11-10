use crate::{
    location_record::record::{LATITUDE, LONGITUDE},
    netcdf::{
        coordinate_facets::CoordinateKey,
        coordinate_role::CoordinateRole,
        error::NetCdfIngestError,
        grid::auxiliary::{AuxiliaryAxis, auxiliary_axes},
        record_keys::{RecordKey, ensure_distinct},
        referenced_variables::referenced_variables,
        value_kind::ValueKind,
    },
};
use derive_more::Display;
use indexmap::IndexMap;
use netcdf_reader::{NcDimension, NcGroup, NcVariable};

/// A latitude or longitude dimension of a grid.
#[derive(Debug, Clone, Copy)]
pub struct HorizontalAxis<'f> {
    /// The dimension's position among the data variables' dimensions.
    pub position: usize,
    /// The dimension's coordinate variable.
    pub coordinate: &'f NcVariable,
}

/// A non-horizontal dimension of a grid, such as time, a vertical level, or an ensemble member.
#[derive(Debug, Clone)]
pub struct FacetAxis<'f> {
    /// The dimension's position among the data variables' dimensions.
    pub position: usize,
    /// The dimension's coordinate variable.
    pub coordinate: &'f NcVariable,
    /// Where the coordinate's value lands in each record.
    pub key: CoordinateKey,
}

/// How a grid's dimensions locate its cells: one latitude, one longitude, and the other dimensions,
/// each through its coordinate variable, plus the scalar and auxiliary coordinates the data variables
/// name.
#[derive(Debug, Clone)]
pub struct GridLayout<'f> {
    /// The data variables' dimensions, in their order.
    pub dimensions: &'f [NcDimension],
    /// The latitude dimension.
    pub latitude: HorizontalAxis<'f>,
    /// The longitude dimension.
    pub longitude: HorizontalAxis<'f>,
    /// Every other dimension, in dimension order.
    pub facets: Vec<FacetAxis<'f>>,
    /// The scalar coordinates and the auxiliary coordinates over the facet dimensions, in the order the
    /// data variables name them.
    pub auxiliaries: Vec<AuxiliaryAxis<'f>>,
}

/// What data variables share to pivot into the same records: their ordered dimensions and the
/// auxiliary coordinates they name, so a 2 m temperature and a 10 m wind on the same grid, told apart
/// only by their scalar `height`, stay in records of their own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GroupKey<'f> {
    /// The names of the dimensions, in order.
    dimensions: Vec<&'f str>,
    /// The names of the auxiliary coordinates, in order.
    auxiliaries: Vec<&'f str>,
}

/// Data variables sharing one ordered list of dimensions, so their latitude-longitude planes line up
/// cell for cell and pivot into columns of the same records.
#[derive(Debug, Clone)]
pub struct GridGroup<'f> {
    /// How the shared dimensions locate each cell.
    pub layout: GridLayout<'f>,
    /// The data variables, in file order.
    pub variables: Vec<&'f NcVariable>,
}

impl GridGroup<'_> {
    /// Fails when two of the group's record keys coincide: a data variable named like a coordinate's
    /// key, or two coordinates emitted under the same key.
    fn ensure_distinct_keys(&self) -> Result<(), NetCdfIngestError> {
        let layout = &self.layout;
        let horizontal = [
            RecordKey::new(LATITUDE, layout.latitude.coordinate.name()),
            RecordKey::new(LONGITUDE, layout.longitude.coordinate.name()),
        ];
        let facets = layout.facets.iter().flat_map(|axis| axis.key.record_keys(axis.coordinate));
        let auxiliaries = layout.auxiliaries.iter().flat_map(|axis| axis.key.record_keys(axis.coordinate));
        let columns = self.variables.iter().map(|variable| RecordKey::new(variable.name(), variable.name()));
        ensure_distinct(horizontal.into_iter().chain(facets).chain(auxiliaries).chain(columns))
    }
}

/// Why a candidate data variable cannot be placed on a latitude-longitude grid.
#[derive(Debug, Display)]
enum SkipReason<'f> {
    /// It holds characters, strings or a user-defined type.
    #[display("it holds characters, strings or a user-defined type rather than numbers")]
    NotNumeric,
    /// One of its dimensions has no coordinate variable.
    #[display("its dimension '{_0}' has no coordinate variable")]
    MissingCoordinate(&'f str),
    /// One of its dimensions' coordinate variables holds a user-defined type.
    #[display("the coordinate variable of its dimension '{_0}' holds a user-defined type")]
    UnreadableCoordinate(&'f str),
    /// Its horizontal axes are projected or rotated.
    #[display("its horizontal axes are projected, so its cells have no latitude and longitude")]
    Projected,
    /// It does not have exactly one latitude and one longitude dimension, as on a curvilinear grid.
    #[display("it does not have exactly one latitude and one longitude dimension")]
    NotOnLatitudeLongitude,
    /// More than one of its dimensions is a time, or more than one a vertical, coordinate.
    #[display("more than one of its dimensions is a time coordinate, or more than one a vertical coordinate")]
    AmbiguousRoles,
}

/// Selects the root group's gridded data variables and groups them by their ordered dimension names
/// and the auxiliary coordinates they name.
///
/// A data variable has at least two dimensions and is neither a coordinate variable nor named by
/// another variable's `bounds`, `climatology` or `grid_mapping`. It qualifies when it is numeric and
/// every dimension has a coordinate variable, exactly one latitude and one longitude, and at most one
/// time and one vertical; a variable that does not is skipped with a warning naming the reason. The
/// scalar and auxiliary coordinates its `coordinates` attribute names become facets of its records too.
/// Subgroups are not read.
///
/// # Errors
///
/// Returns [`NetCdfIngestError::NoRecordVariables`] when no variable qualifies, and
/// [`NetCdfIngestError::ColumnCollision`] when a group's records would carry one key twice.
pub fn grid_groups(root: &NcGroup) -> Result<Vec<GridGroup<'_>>, NetCdfIngestError> {
    if !root.groups.is_empty() {
        tracing::warn!(subgroups = root.groups.len(), "NetCDF subgroups are not read; only the root group is ingested");
    }
    let referenced = referenced_variables(root);

    let mut groups: IndexMap<GroupKey<'_>, GridGroup<'_>> = IndexMap::new();
    let candidates = root
        .variables
        .iter()
        .filter(|variable| variable.ndim() >= 2 && !variable.is_coordinate_variable() && !referenced.contains(variable.name()));
    for variable in candidates {
        match grid_layout(root, variable) {
            Ok(layout) => {
                let key = GroupKey {
                    dimensions: variable.dimensions().iter().map(|dimension| dimension.name.as_str()).collect(),
                    auxiliaries: layout.auxiliaries.iter().map(|axis| axis.coordinate.name()).collect(),
                };
                groups
                    .entry(key)
                    .or_insert_with(|| GridGroup { layout, variables: Vec::new() })
                    .variables
                    .push(variable);
            }
            Err(reason) => tracing::warn!(variable = variable.name(), %reason, "NetCDF variable skipped"),
        }
    }

    let groups: Vec<GridGroup<'_>> = groups.into_values().collect();
    if groups.is_empty() {
        return Err(NetCdfIngestError::NoRecordVariables);
    }
    groups.iter().try_for_each(GridGroup::ensure_distinct_keys)?;
    Ok(groups)
}

/// Resolves how a candidate data variable's dimensions locate its cells.
fn grid_layout<'f>(root: &'f NcGroup, variable: &'f NcVariable) -> Result<GridLayout<'f>, SkipReason<'f>> {
    if ValueKind::of(variable.dtype()) != ValueKind::Numeric {
        return Err(SkipReason::NotNumeric);
    }

    let mut latitude = None;
    let mut longitude = None;
    let mut facets = Vec::new();
    let mut roles = Vec::new();
    for (position, dimension) in variable.dimensions().iter().enumerate() {
        let name = dimension.name.as_str();
        let coordinate = root.coordinate_variable(name).ok_or(SkipReason::MissingCoordinate(name))?;
        if ValueKind::of(coordinate.dtype()) == ValueKind::UserDefined {
            return Err(SkipReason::UnreadableCoordinate(name));
        }
        let role = CoordinateRole::of(coordinate.attributes());
        let horizontal = HorizontalAxis { position, coordinate };
        match role {
            CoordinateRole::Latitude => {
                if latitude.replace(horizontal).is_some() {
                    return Err(SkipReason::NotOnLatitudeLongitude);
                }
            }
            CoordinateRole::Longitude => {
                if longitude.replace(horizontal).is_some() {
                    return Err(SkipReason::NotOnLatitudeLongitude);
                }
            }
            CoordinateRole::ProjectedX | CoordinateRole::ProjectedY => return Err(SkipReason::Projected),
            CoordinateRole::Time | CoordinateRole::Vertical | CoordinateRole::Other => {
                roles.push(role);
                facets.push(FacetAxis {
                    position,
                    coordinate,
                    key: CoordinateKey::of(coordinate, role),
                });
            }
        }
    }

    let (Some(latitude), Some(longitude)) = (latitude, longitude) else {
        return Err(SkipReason::NotOnLatitudeLongitude);
    };
    let count = |wanted: CoordinateRole| roles.iter().filter(|role| **role == wanted).count();
    if count(CoordinateRole::Time) > 1 || count(CoordinateRole::Vertical) > 1 {
        return Err(SkipReason::AmbiguousRoles);
    }
    let auxiliaries = auxiliary_axes(root, variable, variable.dimensions(), &facets);
    Ok(GridLayout {
        dimensions: variable.dimensions(),
        latitude,
        longitude,
        facets,
        auxiliaries,
    })
}
