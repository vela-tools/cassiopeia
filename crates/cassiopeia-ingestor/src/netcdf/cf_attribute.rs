use netcdf_reader::{NcAttrValue, NcAttribute};
use strum::IntoStaticStr;

/// A CF or NUG attribute the ingestor reads, spelled as the convention names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum CfAttribute {
    /// The CF standard name identifying what a variable measures (CF 1.11 §3.3).
    StandardName,
    /// The UDUNITS units of a variable (CF 1.11 §3.1).
    Units,
    /// The `X`, `Y`, `Z` or `T` axis a coordinate represents (CF 1.11 §4).
    Axis,
    /// Whether a vertical coordinate increases `up` or `down` (CF 1.11 §4.3).
    Positive,
    /// The calendar of a time coordinate (CF 1.11 §4.4.2).
    Calendar,
    /// The variable holding a coordinate's cell boundaries (CF 1.11 §7.1).
    Bounds,
    /// The variable holding a climatological time's cell boundaries (CF 1.11 §7.4).
    Climatology,
    /// The grid mapping variable describing a data variable's coordinate reference system (CF 1.11 §5.6).
    GridMapping,
    /// The auxiliary coordinate variables of a data variable (CF 1.11 §5).
    Coordinates,
    /// The role of a variable in a discrete sampling geometry, such as `timeseries_id` (CF 1.11 §9.5).
    CfRole,
    /// The sample dimension a contiguous ragged array's count variable describes (CF 1.11 §9.3.3).
    SampleDimension,
    /// The instance dimension an indexed ragged array's index variable refers to (CF 1.11 §9.3.4).
    InstanceDimension,
    /// The global attribute naming a discrete sampling geometry's feature type (CF 1.11 §9.4).
    #[strum(serialize = "featureType")]
    FeatureType,
    /// The value standing for an unwritten or missing element (NUG, "Attribute Conventions").
    #[strum(serialize = "_FillValue")]
    FillValue,
}

impl CfAttribute {
    /// The attribute's text value in `attributes`, trimmed of surrounding whitespace and the NUL padding
    /// some writers leave in classic character attributes.
    ///
    /// Returns `None` when the attribute is absent or holds numbers rather than text.
    #[must_use]
    pub fn text(self, attributes: &[NcAttribute]) -> Option<&str> {
        let name: &'static str = self.into();
        let text = match &attributes.iter().find(|attribute| attribute.name == name)?.value {
            NcAttrValue::Chars(text) => text.as_str(),
            NcAttrValue::Strings(texts) => match texts.as_slice() {
                [text] => text.as_str(),
                [] | [_, _, ..] => return None,
            },
            NcAttrValue::Bytes(_)
            | NcAttrValue::Shorts(_)
            | NcAttrValue::Ints(_)
            | NcAttrValue::Floats(_)
            | NcAttrValue::Doubles(_)
            | NcAttrValue::UBytes(_)
            | NcAttrValue::UShorts(_)
            | NcAttrValue::UInts(_)
            | NcAttrValue::Int64s(_)
            | NcAttrValue::UInt64s(_)
            | NcAttrValue::UserDefined { .. } => return None,
        };
        Some(text.trim_matches(|character: char| character == '\0' || character.is_whitespace()))
    }

    /// Whether `attributes` declare this attribute, whatever its value.
    #[must_use]
    pub fn is_declared(self, attributes: &[NcAttribute]) -> bool {
        let name: &'static str = self.into();
        attributes.iter().any(|attribute| attribute.name == name)
    }

    /// The variable names a reference attribute lists (`coordinates`, `bounds`, `grid_mapping`, ...).
    ///
    /// The extended `grid_mapping` form (`crs: lat lon`, CF 1.11 §5.6) names its grid mapping variable
    /// with a trailing colon, which is dropped; the coordinate names it pairs with are returned too.
    pub fn names(self, attributes: &[NcAttribute]) -> impl Iterator<Item = &str> {
        self.text(attributes)
            .into_iter()
            .flat_map(str::split_whitespace)
            .map(|name| name.trim_end_matches(':'))
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::cf_attribute::CfAttribute;
    use netcdf_reader::{NcAttrValue, NcAttribute};
    use pretty_assertions::assert_eq;

    fn attribute(name: &str, value: NcAttrValue) -> NcAttribute {
        NcAttribute { name: name.to_string(), value }
    }

    #[test]
    fn a_character_attribute_is_read_trimmed_of_padding() {
        let attributes = [attribute("units", NcAttrValue::Chars(" degrees_north\0\0".to_string()))];
        assert_eq!(CfAttribute::Units.text(&attributes), Some("degrees_north"));
    }

    #[test]
    fn a_single_string_attribute_is_read_as_text() {
        let attributes = [attribute("standard_name", NcAttrValue::Strings(vec!["time".to_string()]))];
        assert_eq!(CfAttribute::StandardName.text(&attributes), Some("time"));
    }

    #[test]
    fn a_numeric_or_absent_attribute_has_no_text() {
        let attributes = [attribute("units", NcAttrValue::Doubles(vec![1.0]))];
        assert_eq!(CfAttribute::Units.text(&attributes), None);
        assert_eq!(CfAttribute::Axis.text(&attributes), None);
    }

    #[test]
    fn the_feature_type_keeps_its_camel_case_spelling() {
        let attributes = [attribute("featureType", NcAttrValue::Chars("timeSeries".to_string()))];
        assert_eq!(CfAttribute::FeatureType.text(&attributes), Some("timeSeries"));
    }

    #[test]
    fn a_numeric_fill_value_is_declared_though_it_has_no_text() {
        let attributes = [attribute("_FillValue", NcAttrValue::Floats(vec![-999.0]))];
        assert!(CfAttribute::FillValue.is_declared(&attributes));
        assert!(!CfAttribute::Units.is_declared(&attributes));
    }

    #[test]
    fn a_reference_attribute_lists_its_names_without_grid_mapping_colons() {
        let attributes = [
            attribute("coordinates", NcAttrValue::Chars("lat lon  alt".to_string())),
            attribute("grid_mapping", NcAttrValue::Chars("crs: lat lon".to_string())),
        ];
        assert_eq!(CfAttribute::Coordinates.names(&attributes).collect::<Vec<_>>(), vec!["lat", "lon", "alt"]);
        assert_eq!(CfAttribute::GridMapping.names(&attributes).collect::<Vec<_>>(), vec!["crs", "lat", "lon"]);
    }
}
