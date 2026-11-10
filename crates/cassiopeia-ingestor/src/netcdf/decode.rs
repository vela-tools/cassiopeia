use crate::{
    netcdf::{default_fill::default_fill, unpacked_type::UnpackedType},
    single_precision::shortest_decimal,
};
use ndarray::ArrayD;
use netcdf_reader::{
    NcFile,
    NcSliceInfo,
    NcVariable,
    masked::MaskParams,
    unpack::{SignParams, UnpackParams},
};
use num_traits::AsPrimitive;

/// How a numeric variable's stored values are decoded into `f64`.
///
/// A value CF 1.11 §8.1 unpacks to `float` is written as its shortest single-precision decimal (see
/// [`shortest_decimal`]); every other value is the double the stored value unpacks to.
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
    ///
    /// Unpacking is the last step and runs in the type the value unpacks to. A single-precision value
    /// is narrowed back to the `float` it was widened from, or that its packed integer exactly is,
    /// unpacked in single precision, and only then rounded to its shortest decimal, so the masks
    /// compare the stored values as they always have.
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
        match UnpackedType::of(variable) {
            UnpackedType::Double => {
                if let Some(unpack) = UnpackParams::from_variable(variable) {
                    unpack.apply(&mut data);
                }
            }
            UnpackedType::Float(packing) => data.mapv_inplace(|value| {
                let stored: f32 = value.as_();
                shortest_decimal(packing.map_or(stored, |packing| packing.unpack(stored)))
            }),
        }
        data
    }
}

#[cfg(test)]
mod tests {
    use crate::netcdf::decode::Decode;
    use netcdf_reader::NcFile;
    use netcdf_writer::{NC_FILL_FLOAT, NC_FILL_INT, NC_FILL_SHORT, NcAttrValue, NcFileBuilder, NcWriteOptions, NcWriteType};
    use pretty_assertions::assert_eq;

    /// Reads back the one variable `v` of a classic file `build` writes.
    fn decoded(decode: Decode, build: impl FnOnce(&mut NcFileBuilder)) -> Vec<f64> {
        let mut builder = NcFileBuilder::new();
        build(&mut builder);
        let (_, bytes) = builder.to_vec(NcWriteOptions::classic()).unwrap();
        let file = NcFile::from_bytes(&bytes).unwrap();
        decode.read(&file, file.variable("v").unwrap()).unwrap()
    }

    fn stored<T: NcWriteType>(values: &'static [T], attributes: Vec<(&'static str, NcAttrValue)>) -> impl FnOnce(&mut NcFileBuilder) {
        move |builder| {
            let dimension = builder.add_dimension("n", u64::try_from(values.len()).unwrap()).unwrap();
            let variable = builder.add_variable::<T>("v", &[dimension]).unwrap();
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

    /// The bits of each value, so values compare exactly.
    fn bits(values: &[f64]) -> Vec<u64> {
        values.iter().map(|value| value.to_bits()).collect()
    }

    #[test]
    fn an_unwritten_value_without_a_fill_value_attribute_is_masked() {
        let values = decoded(Decode::MaskAndUnpack, stored(&[NC_FILL_INT, 5], Vec::new()));
        assert_eq!(masked(&values), vec![true, false]);
        assert_eq!(values[1].to_bits(), 5.0_f64.to_bits());
    }

    #[test]
    fn a_coordinate_read_unpacked_keeps_the_default_fill() {
        let values = decoded(Decode::Unpack, stored(&[NC_FILL_INT, 5], Vec::new()));
        assert_eq!(masked(&values), vec![false, false]);
    }

    #[test]
    fn a_declared_fill_value_masks_its_own_value_and_not_the_default() {
        let values = decoded(
            Decode::MaskAndUnpack,
            stored(&[NC_FILL_INT, -1, 5], vec![("_FillValue", NcAttrValue::Ints(vec![-1]))]),
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
    fn a_float_is_written_as_its_shortest_decimal() {
        let values = decoded(Decode::MaskAndUnpack, stored(&[34.408_f32], Vec::new()));
        assert_eq!(bits(&values), bits(&[34.408]));
    }

    #[test]
    fn a_float_coordinate_is_written_as_its_shortest_decimal() {
        let values = decoded(Decode::Unpack, stored(&[46.2_f32, 14.5], Vec::new()));
        assert_eq!(bits(&values), bits(&[46.2, 14.5]));
    }

    #[test]
    fn a_float_default_fill_is_masked_before_the_value_is_rounded() {
        let values = decoded(Decode::MaskAndUnpack, stored(&[NC_FILL_FLOAT, 1.1_f32], Vec::new()));
        assert_eq!(masked(&values), vec![true, false]);
        assert_eq!(values[1].to_bits(), 1.1_f64.to_bits());
    }

    #[test]
    fn a_short_packed_by_a_float_scale_is_unpacked_in_single_precision() {
        // NOAA OISST stores `sst` as a short with `scale_factor = 0.01f`.
        let values = decoded(
            Decode::MaskAndUnpack,
            stored(&[-167_i16], vec![("scale_factor", NcAttrValue::Floats(vec![0.01]))]),
        );
        assert_eq!(bits(&values), bits(&[-1.67]));
    }

    #[test]
    fn a_short_packed_by_float_attributes_masks_its_stored_fill_value_before_unpacking() {
        let values = decoded(
            Decode::MaskAndUnpack,
            stored(
                &[-999_i16, 1500],
                vec![
                    ("_FillValue", NcAttrValue::Shorts(vec![-999])),
                    ("scale_factor", NcAttrValue::Floats(vec![0.01])),
                    ("add_offset", NcAttrValue::Floats(vec![273.15])),
                ],
            ),
        );
        assert_eq!(masked(&values), vec![true, false]);
        assert_eq!(values[1].to_bits(), 288.15_f64.to_bits());
    }

    #[test]
    fn an_int_packed_by_a_float_scale_is_unpacked_in_double_precision() {
        let values = decoded(
            Decode::MaskAndUnpack,
            stored(&[-167_i32], vec![("scale_factor", NcAttrValue::Floats(vec![0.01]))]),
        );
        assert_eq!(bits(&values), bits(&[-167.0 * f64::from(0.01_f32)]));
    }

    #[test]
    fn a_float_packed_by_a_double_scale_is_unpacked_in_double_precision() {
        let values = decoded(
            Decode::MaskAndUnpack,
            stored(&[1.1_f32], vec![("scale_factor", NcAttrValue::Doubles(vec![2.0]))]),
        );
        assert_eq!(bits(&values), bits(&[f64::from(1.1_f32) * 2.0]));
    }

    #[test]
    fn a_double_keeps_every_digit() {
        let values = decoded(Decode::MaskAndUnpack, stored(&[34.408_000_946_044_92_f64], Vec::new()));
        assert_eq!(bits(&values), bits(&[34.408_000_946_044_92]));
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
