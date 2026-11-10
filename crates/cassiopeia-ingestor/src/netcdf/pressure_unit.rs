use strum::EnumString;

/// A UDUNITS pressure unit a CF vertical coordinate may be expressed in.
///
/// Pressure units identify a vertical coordinate (CF 1.11 §4.3) and are converted to the hectopascals
/// every isobaric level is emitted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString)]
pub enum PressureUnit {
    /// The SI pascal.
    #[strum(serialize = "Pa", serialize = "pascal", serialize = "pascals")]
    Pascal,
    /// The hectopascal, the canonical isobaric level unit.
    #[strum(serialize = "hPa", serialize = "hectopascal", serialize = "hectopascals")]
    Hectopascal,
    /// The millibar, equal to one hectopascal.
    #[strum(serialize = "mbar", serialize = "millibar", serialize = "millibars")]
    Millibar,
}

impl PressureUnit {
    /// The pressure unit `units` name, if they name one.
    #[must_use]
    pub fn parse(units: &str) -> Option<PressureUnit> {
        units.trim().parse().ok()
    }

    /// Converts a value in this unit to hectopascals.
    #[must_use]
    pub const fn to_hectopascals(self, value: f64) -> f64 {
        match self {
            PressureUnit::Pascal => value / 100.0,
            PressureUnit::Hectopascal | PressureUnit::Millibar => value,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::pressure_unit::PressureUnit;
    use pretty_assertions::assert_eq;

    #[test]
    fn pressure_units_parse_from_their_symbols_and_names() {
        assert_eq!(PressureUnit::parse("Pa"), Some(PressureUnit::Pascal));
        assert_eq!(PressureUnit::parse("hPa"), Some(PressureUnit::Hectopascal));
        assert_eq!(PressureUnit::parse("millibar"), Some(PressureUnit::Millibar));
        assert_eq!(PressureUnit::parse("m"), None);
    }

    #[test]
    fn every_pressure_unit_converts_to_hectopascals() {
        assert_eq!(PressureUnit::Pascal.to_hectopascals(85_000.0).to_bits(), 850.0_f64.to_bits());
        assert_eq!(PressureUnit::Hectopascal.to_hectopascals(850.0).to_bits(), 850.0_f64.to_bits());
        assert_eq!(PressureUnit::Millibar.to_hectopascals(500.0).to_bits(), 500.0_f64.to_bits());
    }
}
