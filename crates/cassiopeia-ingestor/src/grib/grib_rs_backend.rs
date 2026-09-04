use crate::grib::{error::GribIngestError, field::DecodedField, grid_slice::GridSliceSet, level::GribLevel, parameter::parameter_key_grib2};
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
        // normalize into the backend-agnostic `GribLevel` (surface type 255 means "no level").
        let level = submessage
            .prod_def()
            .fixed_surfaces()
            .and_then(|(first, _second)| GribLevel::from_grib2(first.surface_type, first.value()));
        // The raw Grid Definition Section bytes identify the grid: byte-equal definitions share
        // geometry and cell ordering, so their values line up cell for cell.
        let grid = submessage.grid_def().iter().copied().collect::<Vec<u8>>();

        // `latlons` yields an owned iterator, so it outlives the decoder that consumes the submessage;
        // both must be drawn before the submessage is moved into the decoder. grib-rs yields single
        // precision, widened losslessly into the field's `f64` currency.
        let latlons: Vec<(f64, f64)> = submessage.latlons()?.map(|(lat, lon)| (f64::from(lat), f64::from(lon))).collect();
        let decoder = Grib2SubmessageDecoder::from(submessage)?;
        let values: Vec<f64> = decoder.dispatch()?.map(f64::from).collect();

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
