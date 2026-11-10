use crate::netcdf::coordinate_role::CoordinateRole;

/// Errors that can occur while ingesting a `NetCDF` file.
#[derive(Debug, thiserror::Error)]
pub enum NetCdfIngestError {
    /// The `NetCDF` ingestor was handed an in-memory byte payload; the reader opens and maps a file.
    #[error("the NetCDF ingestor requires a file, not bytes")]
    RequiresFile,

    /// The reader failed to open the file, read a variable, or decode a time value.
    #[error(transparent)]
    Read(#[from] netcdf_reader::Error),

    /// The global `featureType` names neither a grid nor `timeSeries`, such as another discrete
    /// sampling geometry (CF 1.11 §9.4).
    #[error("the NetCDF featureType '{feature_type}' is not supported; only gridded files and timeSeries files are")]
    UnsupportedFeatureType { feature_type: String },

    /// No data variable could be placed on latitude and longitude, so the file holds no records.
    #[error("the NetCDF file holds no data variable located by latitude and longitude")]
    NoRecordVariables,

    /// A time coordinate's `units` are not ASCII, so they cannot be CF time units.
    #[error("the units of the NetCDF time variable '{variable}' are not ASCII")]
    NonAsciiTimeUnits { variable: String },

    /// A time coordinate names a calendar the decoder does not implement: not one of the CF 1.11 §4.4.2
    /// names, or `utc`, `tai` or `none`, whose leap-second or calendar-free arithmetic it lacks.
    #[error("the NetCDF time variable '{variable}' uses the calendar '{calendar}', which Cassiopeia cannot decode")]
    UnknownCalendar { variable: String, calendar: String },

    /// A time coordinate's epoch carries a UTC offset other than zero.
    #[error("the units '{units}' of the NetCDF time variable '{variable}' name a time zone other than UTC")]
    UnsupportedTimeZone { variable: String, units: String },

    /// A variable would be emitted under a record key another variable or coordinate already uses.
    #[error("the NetCDF variable '{variable}' would be emitted under a record key another variable already uses")]
    ColumnCollision { variable: String },

    /// A ragged array's counts or indices point outside the dimension they describe (CF 1.11 §9.3.3,
    /// §9.3.4).
    #[error("the NetCDF ragged array variable '{variable}' points outside its dimension")]
    InvalidRaggedArray { variable: String },

    /// A `timeSeries` file has no coordinate in the given role.
    #[error("the NetCDF timeSeries file has no {role} coordinate")]
    MissingTimeSeriesCoordinate { role: CoordinateRole },

    /// A `timeSeries` file's coordinates match none of the CF 1.11 Appendix H.2 layouts.
    #[error("the coordinates of the NetCDF timeSeries file match none of the CF timeSeries layouts")]
    UnrecognizedTimeSeriesLayout,

    /// A dimension is longer than this platform can index.
    #[error("the NetCDF dimension '{dimension}' is too long to index on this platform")]
    DimensionTooLong { dimension: String },
}
