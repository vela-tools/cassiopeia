use crate::{
    grib::{error::GribIngestError, field::DecodedField, grid_slice::GridSliceSet, level::grib2_level, parameter::parameter_key_grib2},
    single_precision::shortest_decimal,
};
use grib::{Grib2SubmessageDecoder, LatLons};
use std::{fs::File, io::BufReader};

/// Decodes a GRIB2 stream with grib-rs, folding each submessage into `set` as a [`DecodedField`].
///
/// This is the pure-Rust GRIB2 fallback, reached when the `grib2-full` feature is off. It has no C
/// dependency but computes coordinates only for regular latitude/longitude grids; a projected grid
/// fails here, where ecCodes (the `grib2-full` path) would succeed.
///
/// A GRIB2 submessage is one 2D field for a single parameter at one level and forecast time.
/// Parameters sharing a grid, level, and time are grouped by `set` into columns of one location
/// record. Submessages whose product-definition template carries no parameter identity contribute no
/// column and are skipped.
///
/// # Errors
///
/// Returns [`GribIngestError::Parse`] if the stream is not valid GRIB2 or a submessage cannot be
/// decoded.
pub fn decode(reader: BufReader<File>, set: &mut GridSliceSet) -> Result<(), GribIngestError> {
    let grib2 = grib::from_reader(reader)?;

    for (_index, submessage) in &grib2 {
        let discipline = submessage.indicator().discipline;
        let (Some(category), Some(number)) = (submessage.prod_def().parameter_category(), submessage.prod_def().parameter_number()) else {
            // The product definition template carries no parameter identity; there is nothing to
            // pivot, so this submessage contributes no column.
            continue;
        };

        // grib-rs exposes only the GRIB2 identity triple, so the canonical key comes from it: the same
        // resolver ecCodes' GRIB2 path uses, which is what guarantees cross-backend parity.
        let parameter = parameter_key_grib2(discipline, category, number);
        let temporal = submessage.temporal_info();
        let reference_time = temporal.ref_time.map(|time| time.to_rfc3339());
        let forecast_time = temporal.forecast_time_target.map(|time| time.to_rfc3339());
        // The first fixed surface is the vertical level; its Code Table 4.5 type and decoded value
        // normalize into the backend-agnostic `Level` (surface type 255 means "no level").
        let level = submessage
            .prod_def()
            .fixed_surfaces()
            .and_then(|(first, _second)| grib2_level(first.surface_type, first.value()));
        // The raw Grid Definition Section bytes identify the grid: byte-equal definitions share
        // geometry and cell ordering, so their values line up cell for cell.
        let grid = submessage.grid_def().iter().copied().collect::<Vec<u8>>();

        // `latlons` yields an owned iterator, so it outlives the decoder that consumes the submessage;
        // both must be drawn before the submessage is moved into the decoder. grib-rs yields
        // coordinates in double precision but values in single precision, so each value enters the
        // field's `f64` currency as its shortest decimal rather than its binary expansion.
        let latlons: Vec<(f64, f64)> = submessage.latlons()?.collect();
        let decoder = Grib2SubmessageDecoder::from(submessage)?;
        let values: Vec<f64> = decoder.dispatch()?.map(shortest_decimal).collect();

        set.add(DecodedField {
            grid,
            parameter,
            latlons,
            values,
            level,
            reference_time,
            forecast_time,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::grib::{
        fixture::{Field, drain, grib2_file},
        grib_rs_backend::decode,
        grid_slice::GridSliceSet,
    };
    use pretty_assertions::assert_eq;
    use serde_json::Value;
    use std::{fs::File, io::BufReader};
    use temp_dir::TempDir;

    /// Decodes one 2x2 temperature field holding `data` and returns its records' temperatures.
    fn temperatures(data: Vec<f64>) -> Vec<Option<f64>> {
        let dir = TempDir::new().unwrap();
        let path = grib2_file(
            &dir,
            &[Field {
                category: 0,
                number: 0,
                ni: 2,
                nj: 2,
                forecast_hours: 6,
                data,
            }],
        );
        let mut set = GridSliceSet::new();
        decode(BufReader::new(File::open(path).unwrap()), &mut set).unwrap();
        drain(set)
            .iter()
            .map(|record| record.data().get("temperature").and_then(Value::as_f64))
            .collect()
    }

    #[test]
    fn a_single_precision_value_is_written_as_its_shortest_decimal() {
        assert_eq!(
            temperatures(vec![280.12, 281.37, 279.05, 283.0]),
            vec![Some(280.12), Some(281.37), Some(279.05), Some(283.0)]
        );
    }

    #[test]
    fn a_masked_value_stays_masked() {
        assert_eq!(
            temperatures(vec![280.12, f64::NAN, 279.05, 283.0]),
            vec![Some(280.12), Some(279.05), Some(283.0)]
        );
    }
}
