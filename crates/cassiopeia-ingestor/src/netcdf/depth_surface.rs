use crate::{location_record::level::LevelType, netcdf::cf_attribute::CfAttribute};
use netcdf_reader::{NcGroup, NcVariable};

/// The surface a CF depth coordinate is measured down from.
///
/// CF's `depth` standard name is "the vertical distance below the surface" (CF standard name table),
/// and a coordinate whose only vertical marker is `positive = "down"` (CF 1.11 §4.3) names no surface
/// either. Land models measure soil layers down from the ground just as ocean models measure the water
/// column down from the sea, so the coordinate alone cannot tell them apart: what the data variables on
/// it hold does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthSurface {
    /// The land surface, which soil quantities are measured below.
    Land,
    /// The sea surface, which sea water quantities are measured below.
    Sea,
}

impl DepthSurface {
    /// The surface the variables `coordinate` locates are measured below, told by their standard names,
    /// or `None` when no standard name tells one or two tell different surfaces.
    ///
    /// A variable without a standard name, such as a quality flag beside a measurement, or with one that
    /// names neither soil nor sea water, tells nothing and does not count against the others.
    #[must_use]
    pub fn of(root: &NcGroup, coordinate: &NcVariable) -> Option<DepthSurface> {
        let mut told = root
            .variables
            .iter()
            .filter(|variable| locates(coordinate, variable))
            .filter_map(|variable| CfAttribute::StandardName.text(variable.attributes()))
            .filter_map(DepthSurface::of_quantity);
        let first = told.next()?;
        told.all(|surface| surface == first).then_some(first)
    }

    /// The level type of a depth below this surface.
    #[must_use]
    pub const fn level_type(self) -> LevelType {
        match self {
            DepthSurface::Land => LevelType::DepthBelowLand,
            DepthSurface::Sea => LevelType::DepthBelowSea,
        }
    }

    /// The surface a quantity is measured below, told by the medium its CF standard name puts it in, or
    /// `None` for a name that puts it in neither soil nor sea water.
    ///
    /// A CF standard name names its medium as words of the name: `soil_temperature` and
    /// `mass_content_of_water_in_soil_layer` are in the soil, `sea_water_salinity`,
    /// `mole_concentration_of_nitrate_in_sea_water` and `ocean_mixed_layer_thickness` in the sea. No name
    /// in version 95 of the CF standard name table, aliases included, holds both. The word `sea` alone
    /// is no medium: `sea_ice_temperature` is measured in the ice and `sea_salt` aerosol in the air. Only
    /// the name's first word counts, since a modifier may follow it after whitespace (CF 1.11 §3.3).
    fn of_quantity(standard_name: &str) -> Option<DepthSurface> {
        let name = standard_name.split_whitespace().next()?;
        let words = || name.split('_');
        let soil = words().any(|word| word == "soil");
        let sea = words().any(|word| word == "ocean") || words().zip(words().skip(1)).any(|pair| pair == ("sea", "water"));
        match (soil, sea) {
            (true, false) => Some(DepthSurface::Land),
            (false, true) => Some(DepthSurface::Sea),
            (true, true) | (false, false) => None,
        }
    }
}

/// Whether `coordinate` locates `variable`: the variable spans the dimension `coordinate` is the
/// coordinate variable of, or names `coordinate` in its `coordinates` attribute (CF 1.11 §5).
fn locates(coordinate: &NcVariable, variable: &NcVariable) -> bool {
    if variable.name() == coordinate.name() {
        return false;
    }
    let spans = coordinate
        .coordinate_dimension()
        .is_some_and(|axis| variable.dimensions().iter().any(|dimension| dimension.name == axis.name));
    spans || CfAttribute::Coordinates.names(variable.attributes()).any(|name| name == coordinate.name())
}

#[cfg(test)]
mod tests {
    use crate::{location_record::level::LevelType, netcdf::depth_surface::DepthSurface};
    use netcdf_reader::{NcAttrValue, NcAttribute, NcDimension, NcGroup, NcType, NcVariable};
    use pretty_assertions::assert_eq;
    use std::iter;

    /// A double variable over the dimensions named `dimensions`, carrying character `attributes`.
    fn variable(name: &str, dimensions: &[&str], attributes: &[(&str, &str)]) -> NcVariable {
        NcVariable {
            name: name.to_string(),
            dimensions: dimensions
                .iter()
                .map(|dimension| NcDimension {
                    name: (*dimension).to_string(),
                    size: 2,
                    is_unlimited: false,
                })
                .collect(),
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

    /// A root group holding a `depth(depth)` coordinate variable and `variables`, with the surface the
    /// variables tell the coordinate is measured below.
    fn surface_of_depth(variables: Vec<NcVariable>) -> Option<DepthSurface> {
        let depth = variable("depth", &["depth"], &[("standard_name", "depth"), ("units", "m"), ("positive", "down")]);
        let root = NcGroup {
            name: "/".to_string(),
            dimensions: Vec::new(),
            variables: iter::once(depth).chain(variables).collect(),
            attributes: Vec::new(),
            groups: Vec::new(),
        };
        DepthSurface::of(&root, &root.variables[0])
    }

    /// A variable over `depth`, `lat` and `lon` declaring `standard_name`.
    fn on_depth(name: &str, standard_name: &str) -> NcVariable {
        variable(name, &["depth", "lat", "lon"], &[("standard_name", standard_name)])
    }

    #[test]
    fn soil_names_are_measured_below_the_land_and_sea_water_names_below_the_sea() {
        assert_eq!(DepthSurface::of_quantity("soil_temperature"), Some(DepthSurface::Land));
        assert_eq!(DepthSurface::of_quantity("mass_content_of_water_in_soil_layer"), Some(DepthSurface::Land));
        assert_eq!(DepthSurface::of_quantity("moisture_content_of_soil_layer"), Some(DepthSurface::Land));
        assert_eq!(DepthSurface::of_quantity("sea_water_potential_temperature"), Some(DepthSurface::Sea));
        assert_eq!(DepthSurface::of_quantity("sea_water_salinity"), Some(DepthSurface::Sea));
        assert_eq!(DepthSurface::of_quantity("mole_concentration_of_nitrate_in_sea_water"), Some(DepthSurface::Sea));
        assert_eq!(DepthSurface::of_quantity("ocean_mixed_layer_thickness"), Some(DepthSurface::Sea));
    }

    #[test]
    fn a_name_putting_its_quantity_in_neither_soil_nor_sea_water_tells_no_surface() {
        assert_eq!(DepthSurface::of_quantity("sea_ice_temperature"), None);
        assert_eq!(DepthSurface::of_quantity("mass_concentration_of_sea_salt_dry_aerosol_particles_in_air"), None);
        assert_eq!(DepthSurface::of_quantity("temperature_in_ground"), None);
        assert_eq!(DepthSurface::of_quantity("cell_thickness"), None);
        assert_eq!(DepthSurface::of_quantity(""), None);
    }

    #[test]
    fn a_modifier_after_the_name_is_ignored() {
        assert_eq!(DepthSurface::of_quantity("soil_temperature standard_error"), Some(DepthSurface::Land));
        assert_eq!(DepthSurface::of_quantity("sea_water_temperature status_flag"), Some(DepthSurface::Sea));
    }

    #[test]
    fn a_depth_under_soil_quantities_is_measured_below_the_land() {
        let tsl = on_depth("tsl", "soil_temperature");
        let mrsol = on_depth("mrsol", "mass_content_of_water_in_soil_layer");
        assert_eq!(surface_of_depth(vec![tsl, mrsol]), Some(DepthSurface::Land));
    }

    #[test]
    fn a_depth_under_sea_quantities_and_a_flag_without_a_standard_name_is_measured_below_the_sea() {
        let temperature = on_depth("TEMP", "sea_water_temperature");
        let flag = variable("TEMP_QC", &["depth", "lat", "lon"], &[("long_name", "quality flag")]);
        let thickness = on_depth("thkcello", "cell_thickness");
        assert_eq!(surface_of_depth(vec![temperature, flag, thickness]), Some(DepthSurface::Sea));
    }

    #[test]
    fn a_depth_under_soil_and_sea_quantities_alike_is_measured_below_no_known_surface() {
        let soil = on_depth("tsl", "soil_temperature");
        let sea = on_depth("thetao", "sea_water_potential_temperature");
        assert_eq!(surface_of_depth(vec![soil, sea]), None);
    }

    #[test]
    fn a_depth_whose_variables_declare_no_standard_name_is_measured_below_no_known_surface() {
        let tsl = variable("tsl", &["depth", "lat", "lon"], &[("units", "K")]);
        assert_eq!(surface_of_depth(vec![tsl]), None);
    }

    #[test]
    fn only_the_variables_the_depth_locates_tell_its_surface() {
        let soil = on_depth("tsl", "soil_temperature");
        let elsewhere = variable("tos", &["lat", "lon"], &[("standard_name", "sea_water_temperature")]);
        assert_eq!(surface_of_depth(vec![soil, elsewhere]), Some(DepthSurface::Land));
    }

    #[test]
    fn a_scalar_depth_tells_the_surface_of_the_variables_naming_it_in_their_coordinates() {
        let depth = variable("sdepth", &[], &[("standard_name", "depth"), ("units", "m")]);
        let named = variable("tsl", &["lat", "lon"], &[("standard_name", "soil_temperature"), ("coordinates", "sdepth")]);
        let unnamed = variable("tos", &["lat", "lon"], &[("standard_name", "sea_water_temperature")]);
        let root = NcGroup {
            name: "/".to_string(),
            dimensions: Vec::new(),
            variables: vec![depth, named, unnamed],
            attributes: Vec::new(),
            groups: Vec::new(),
        };
        assert_eq!(DepthSurface::of(&root, &root.variables[0]), Some(DepthSurface::Land));
    }

    #[test]
    fn each_surface_has_its_depth_level_type() {
        assert_eq!(DepthSurface::Land.level_type(), LevelType::DepthBelowLand);
        assert_eq!(DepthSurface::Sea.level_type(), LevelType::DepthBelowSea);
    }
}
