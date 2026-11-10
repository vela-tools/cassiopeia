use crate::netcdf::cf_attribute::CfAttribute;
use netcdf_reader::NcGroup;
use std::collections::HashSet;

/// The attributes that name auxiliary variables rather than data: cell boundaries and grid mappings.
const REFERENCE_ATTRIBUTES: [CfAttribute; 3] = [CfAttribute::Bounds, CfAttribute::Climatology, CfAttribute::GridMapping];

/// The names of the variables a group's variables reference as cell boundaries (CF 1.11 §7.1, §7.4)
/// or grid mappings (§5.6). They describe other variables, so they are never data themselves.
#[must_use]
pub fn referenced_variables(group: &NcGroup) -> HashSet<&str> {
    group
        .variables
        .iter()
        .flat_map(|variable| REFERENCE_ATTRIBUTES.into_iter().flat_map(|attribute| attribute.names(variable.attributes())))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::netcdf::referenced_variables::referenced_variables;
    use netcdf_reader::{NcAttrValue, NcAttribute, NcGroup, NcType, NcVariable};
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    fn variable(name: &str, attributes: &[(&str, &str)]) -> NcVariable {
        NcVariable {
            name: name.to_string(),
            dimensions: Vec::new(),
            dtype: NcType::Double,
            attributes: attributes
                .iter()
                .map(|(name, value)| NcAttribute {
                    name: (*name).to_string(),
                    value: NcAttrValue::Chars((*value).to_string()),
                })
                .collect(),
            data_offset: 0,
            _data_size: 0,
            is_record_var: false,
            record_size: 0,
        }
    }

    #[test]
    fn bounds_climatology_and_grid_mapping_targets_are_referenced_but_coordinates_are_not() {
        let group = NcGroup {
            name: "/".to_string(),
            dimensions: Vec::new(),
            variables: vec![
                variable("time", &[("bounds", "time_bnds"), ("climatology", "climatology_bounds")]),
                variable("tas", &[("grid_mapping", "crs"), ("coordinates", "height")]),
            ],
            attributes: Vec::new(),
            groups: Vec::new(),
        };
        assert_eq!(referenced_variables(&group), HashSet::from(["time_bnds", "climatology_bounds", "crs"]));
    }
}
