use crate::location_record::{level::Level, parameter_name::ParameterName};

/// One decoded GRIB field: a single 2D parameter grid with its coordinates, vertical level, and times.
///
/// This is the edition-agnostic seam between decoding and the pivot. Both decoders (grib-rs for
/// GRIB2, ecCodes for GRIB1) produce this same shape, and everything downstream (the grid-slice
/// pivot, record emission) is identical for both editions.
///
/// Coordinates and values are `f64`: ecCodes decodes both editions in double precision, and grib-rs
/// decodes GRIB2 values in single precision, each carried as the double nearest its shortest decimal
/// (see [`crate::single_precision::shortest_decimal`]), so no narrowing cast is needed on either path.
pub struct DecodedField {
    /// A stable per-grid identity: the raw Grid Definition Section bytes for GRIB2, a grid-section
    /// hash for GRIB1. Byte-equal identities share geometry and cell ordering, so values line up cell
    /// for cell across fields.
    pub grid: Vec<u8>,
    /// The parameter this field measures, used as the record column key.
    pub parameter: ParameterName,
    /// Per-cell `(latitude, longitude)` in the grid's scanning order, aligned index-for-index with
    /// `values`. Longitude is always in `[-180, 180)`, west negative; see
    /// [`crate::location_record::longitude::normalize_longitude`].
    pub latlons: Vec<(f64, f64)>,
    /// Per-cell values in the same order as `latlons`; bitmap-masked cells are `NaN`.
    pub values: Vec<f64>,
    /// The normalized vertical level, when the field carries one.
    pub level: Option<Level>,
    /// RFC 3339 reference (analysis) time, when present.
    pub reference_time: Option<String>,
    /// RFC 3339 forecast valid time, when present.
    pub forecast_time: Option<String>,
}
