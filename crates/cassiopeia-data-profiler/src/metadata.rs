use crate::inspectors::csv::dialect::Dialect;
use serde::Serialize;

/// Format-specific details discovered while profiling a payload.
#[derive(Debug, Clone, Serialize)]
pub enum FormatMetadata {
    /// Dialect details required to parse a CSV payload.
    Csv(CsvMetadata),
    /// The GRIB edition inspected from a gridded binary payload.
    Grib(GribMetadata),
}

impl FormatMetadata {
    /// Returns the CSV metadata, or `None` for a different format.
    #[must_use]
    pub const fn as_csv(&self) -> Option<&CsvMetadata> {
        match self {
            Self::Csv(metadata) => Some(metadata),
            Self::Grib(_) => None,
        }
    }

    /// Returns the GRIB metadata, or `None` for a different format.
    #[must_use]
    pub const fn as_grib(&self) -> Option<&GribMetadata> {
        match self {
            Self::Grib(metadata) => Some(metadata),
            Self::Csv(_) => None,
        }
    }
}

/// The GRIB edition a gridded binary payload declares in its Indicator Section.
///
/// GRIB1 and GRIB2 share the `GRIB` magic but are structurally distinct formats decoded by different
/// code paths; the profiler inspects the edition octet so the ingestor can dispatch without re-reading
/// the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GribEdition {
    /// GRIB edition 1 (WMO FM 92 GRIB, first edition).
    V1,
    /// GRIB edition 2 (WMO FM 92 GRIB, second edition).
    V2,
}

/// Metadata discovered for a GRIB payload: the edition read from its Indicator Section.
#[derive(Debug, Clone, Serialize)]
pub struct GribMetadata {
    edition: GribEdition,
}

impl GribMetadata {
    /// Builds GRIB metadata from an inspected edition.
    #[must_use]
    pub const fn new(edition: GribEdition) -> Self {
        Self { edition }
    }

    /// Returns the GRIB edition inspected from the payload.
    #[must_use]
    pub const fn edition(&self) -> GribEdition {
        self.edition
    }
}

/// Metadata discovered for a CSV payload.
#[derive(Debug, Clone, Serialize)]
pub struct CsvMetadata {
    dialect: Dialect,
}

impl CsvMetadata {
    /// Builds CSV metadata from a detected dialect.
    #[must_use]
    pub const fn new(dialect: Dialect) -> Self {
        Self { dialect }
    }

    /// Returns the dialect used to parse the CSV payload.
    #[must_use]
    pub const fn dialect(&self) -> &Dialect {
        &self.dialect
    }
}
