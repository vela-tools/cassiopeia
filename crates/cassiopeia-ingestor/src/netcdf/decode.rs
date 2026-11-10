use crate::netcdf::default_fill::default_fill;
use ndarray::ArrayD;
use netcdf_reader::{
    NcFile,
    NcSliceInfo,
    NcVariable,
    masked::MaskParams,
    unpack::{SignParams, UnpackParams},
};

/// How a numeric variable's stored values are decoded into `f64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decode {
    /// `_Unsigned`, `scale_factor` and `add_offset` applied, nothing masked: a coordinate variable
    /// may not hold missing values (CF 1.11 §5), so its `valid_range` is not allowed to drop cells.
    Unpack,
    /// The netCDF default fill value of a variable without `_FillValue`, then `_FillValue`,
    /// `missing_value` and the valid range, masked to `NaN` before unpacking, in the order CF 1.11
    /// §8.1 gives.
    MaskAndUnpack,
}

impl Decode {
    /// Reads every value of `variable`, flattened in row-major order.
    ///
    /// # Errors
    ///
    /// Returns the reader's error when the variable cannot be read or holds no numbers.
    pub fn read(self, file: &NcFile, variable: &NcVariable) -> Result<Vec<f64>, netcdf_reader::Error> {
        let values = self.decode(variable, file.read_variable_as_f64(variable.name())?);
        Ok(values.into_iter().collect())
    }

    /// Reads the hyperslab `selection` of `variable`, its indexed dimensions dropped.
    ///
    /// # Errors
    ///
    /// Returns the reader's error when the slice cannot be read or the variable holds no numbers.
    pub fn read_slice(self, file: &NcFile, variable: &NcVariable, selection: &NcSliceInfo) -> Result<ArrayD<f64>, netcdf_reader::Error> {
        Ok(self.decode(variable, file.read_variable_slice_as_f64(variable.name(), selection)?))
    }

    /// Decodes stored values promoted to `f64`.
    ///
    /// The reader's own masked reads compare only against the attributes a variable declares, so the
    /// steps are composed here from its public parameters, with the default fill step added. That
    /// step compares the stored value, so it runs before `_Unsigned` reinterprets it and before any
    /// unpacking, and a masked value stays `NaN` through both.
    fn decode(self, variable: &NcVariable, mut data: ArrayD<f64>) -> ArrayD<f64> {
        if self == Decode::MaskAndUnpack
            && let Some(fill) = default_fill(variable)
        {
            data.mapv_inplace(|value| if value.to_bits() == fill.to_bits() { f64::NAN } else { value });
        }
        if let Some(sign) = SignParams::from_variable(variable) {
            sign.apply(&mut data);
        }
        if self == Decode::MaskAndUnpack
            && let Some(mask) = MaskParams::from_variable(variable)
        {
            mask.apply(&mut data);
        }
        if let Some(unpack) = UnpackParams::from_variable(variable) {
            unpack.apply(&mut data);
        }
        data
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::decode::Decode;
    use netcdf_reader::NcFile;
    use netcdf_writer::{NC_FILL_INT, NC_FILL_SHORT, NcAttrValue, NcFileBuilder, NcWriteOptions};
    use pretty_assertions::assert_eq;

    /// Reads back the one variable `v` of a classic file `build` writes.
    fn decoded(decode: Decode, build: impl FnOnce(&mut NcFileBuilder)) -> Vec<f64> {
        let mut builder = NcFileBuilder::new();
        build(&mut builder);
        let (_, bytes) = builder.to_vec(NcWriteOptions::classic()).unwrap();
        let file = NcFile::from_bytes(&bytes).unwrap();
        decode.read(&file, file.variable("v").unwrap()).unwrap()
    }

    fn integers(values: &'static [i32], attributes: Vec<(&'static str, NcAttrValue)>) -> impl FnOnce(&mut NcFileBuilder) {
        move |builder| {
            let dimension = builder.add_dimension("n", u64::try_from(values.len()).unwrap()).unwrap();
            let variable = builder.add_variable::<i32>("v", &[dimension]).unwrap();
            for (name, value) in attributes {
                builder.add_variable_attribute(variable, name, value).unwrap();
            }
            builder.write_variable(variable, values).unwrap();
        }
    }

    /// Whether each value is masked, so `NaN` compares.
    fn masked(values: &[f64]) -> Vec<bool> {
        values.iter().map(|value| value.is_nan()).collect()
    }

    #[test]
    fn an_unwritten_value_without_a_fill_value_attribute_is_masked() {
        let values = decoded(Decode::MaskAndUnpack, integers(&[NC_FILL_INT, 5], Vec::new()));
        assert_eq!(masked(&values), vec![true, false]);
        assert_eq!(values[1].to_bits(), 5.0_f64.to_bits());
    }

    #[test]
    fn a_coordinate_read_unpacked_keeps_the_default_fill() {
        let values = decoded(Decode::Unpack, integers(&[NC_FILL_INT, 5], Vec::new()));
        assert_eq!(masked(&values), vec![false, false]);
    }

    #[test]
    fn a_declared_fill_value_masks_its_own_value_and_not_the_default() {
        let values = decoded(
            Decode::MaskAndUnpack,
            integers(&[NC_FILL_INT, -1, 5], vec![("_FillValue", NcAttrValue::Ints(vec![-1]))]),
        );
        assert_eq!(masked(&values), vec![false, true, false]);
    }

    #[test]
    fn a_packed_default_fill_is_masked_before_it_is_unpacked() {
        let values = decoded(Decode::MaskAndUnpack, |builder| {
            let dimension = builder.add_dimension("n", 2).unwrap();
            let variable = builder.add_variable::<i16>("v", &[dimension]).unwrap();
            builder
                .add_variable_attribute(variable, "scale_factor", NcAttrValue::Doubles(vec![0.5]))
                .unwrap();
            builder
                .add_variable_attribute(variable, "add_offset", NcAttrValue::Doubles(vec![10.0]))
                .unwrap();
            builder.write_variable(variable, &[NC_FILL_SHORT, 4]).unwrap();
        });
        assert_eq!(masked(&values), vec![true, false]);
        assert_eq!(values[1].to_bits(), 12.0_f64.to_bits());
    }

    #[test]
    fn a_byte_at_its_default_fill_is_real_data() {
        let values = decoded(Decode::MaskAndUnpack, |builder| {
            let dimension = builder.add_dimension("n", 1).unwrap();
            let variable = builder.add_variable::<i8>("v", &[dimension]).unwrap();
            builder.write_variable(variable, &[-127_i8]).unwrap();
        });
        assert_eq!(values[0].to_bits(), (-127.0_f64).to_bits());
    }
}
