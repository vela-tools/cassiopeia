use crate::{content::Content, detectors::Detector, profile::Profile};
use cassiopeia_common::format::DataFormat;
use mediatype::{MediaType, Name, names::APPLICATION};
use std::iter;

/// The unregistered but conventional media type for `NetCDF`, `application/x-netcdf`.
const NETCDF_MEDIA_TYPE: MediaType<'static> = MediaType::new(APPLICATION, Name::new_unchecked("x-netcdf"));

/// The three octets every classic `NetCDF` file opens with, ahead of its version byte.
const CLASSIC_MAGIC: &[u8; 3] = b"CDF";

/// The classic version bytes: 1 for CDF-1 (classic), 2 for CDF-2 (64-bit offset), 5 for CDF-5 (64-bit
/// data).
const CLASSIC_VERSIONS: [u8; 3] = [1, 2, 5];

/// The eight-octet HDF5 format signature that opens the superblock of a `NetCDF-4` file.
const HDF5_SIGNATURE: &[u8; 8] = b"\x89HDF\r\n\x1a\n";

/// The first offset past the start where an HDF5 superblock may sit behind a user block.
const FIRST_USER_BLOCK_OFFSET: usize = 512;

/// Detects `NetCDF` from its file signature.
///
/// A classic file opens with `CDF` and a version byte of 1, 2 or 5 (`NetCDF` classic format
/// specification). A `NetCDF-4` file is an HDF5 file, whose superblock signature sits at offset 0 or,
/// behind a user block, at 512 or any doubling of it (HDF5 File Format Specification, section II.A),
/// the same offsets netCDF-C and the reader search. Every HDF5 file is claimed: the `NetCDF-4` data
/// model is the only HDF5 dialect Cassiopeia reads, and the reader reports a plain HDF5 file it cannot
/// interpret. The variables a file holds are discovered by the ingestor, so no metadata is attached.
pub struct NetCdfDetector;

impl Detector for NetCdfDetector {
    fn detect(&self, content: &Content<'_>) -> Option<Profile> {
        let bytes = content.bytes();
        (is_classic(bytes) || is_hdf5(bytes)).then(|| Profile::new(DataFormat::NetCdf, NETCDF_MEDIA_TYPE, 1.0))
    }
}

/// Whether `bytes` open with the classic `CDF` magic and a known version byte.
fn is_classic(bytes: &[u8]) -> bool {
    match bytes {
        [first, second, third, version, ..] => [*first, *second, *third] == *CLASSIC_MAGIC && CLASSIC_VERSIONS.contains(version),
        [] | [_] | [_, _] | [_, _, _] => false,
    }
}

/// Whether the HDF5 signature sits at offset 0 or at any user-block offset that still fits in `bytes`.
fn is_hdf5(bytes: &[u8]) -> bool {
    iter::once(0)
        .chain(iter::successors(Some(FIRST_USER_BLOCK_OFFSET), |offset| offset.checked_mul(2)))
        .map_while(|offset| bytes.get(offset..offset.checked_add(HDF5_SIGNATURE.len())?))
        .any(|window| window == HDF5_SIGNATURE)
}

#[cfg(test)]
mod tests {
    use crate::{
        content::Content,
        detectors::{Detector, netcdf::NetCdfDetector},
    };
    use cassiopeia_common::format::DataFormat;
    use pretty_assertions::assert_eq;

    const HDF5_SIGNATURE: &[u8] = b"\x89HDF\r\n\x1a\n";

    /// `length` zero octets with the HDF5 signature written at `offset`.
    fn hdf5_at(offset: usize, length: usize) -> Vec<u8> {
        let mut bytes = vec![0; length];
        bytes[offset..offset + HDF5_SIGNATURE.len()].copy_from_slice(HDF5_SIGNATURE);
        bytes
    }

    #[test]
    fn the_three_classic_versions_are_detected_as_netcdf() {
        for version in [1u8, 2, 5] {
            let profile = NetCdfDetector.detect(&Content::new(&[b'C', b'D', b'F', version, 0, 0, 0, 0])).unwrap();
            assert_eq!(*profile.format(), DataFormat::NetCdf);
            assert_eq!(profile.mime_type().to_string(), "application/x-netcdf");
            assert!((*profile.confidence() - 1.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn an_unknown_classic_version_is_not_netcdf() {
        assert!(NetCdfDetector.detect(&Content::new(b"CDF\x03\x00\x00\x00\x00")).is_none());
    }

    #[test]
    fn an_hdf5_signature_at_the_start_is_detected_as_netcdf() {
        let profile = NetCdfDetector.detect(&Content::new(&hdf5_at(0, 64))).unwrap();
        assert_eq!(*profile.format(), DataFormat::NetCdf);
    }

    #[test]
    fn an_hdf5_signature_behind_a_user_block_is_detected_as_netcdf() {
        assert!(NetCdfDetector.detect(&Content::new(&hdf5_at(512, 1024))).is_some());
        assert!(NetCdfDetector.detect(&Content::new(&hdf5_at(2048, 2056))).is_some());
    }

    #[test]
    fn an_hdf5_signature_off_the_user_block_offsets_is_not_netcdf() {
        assert!(NetCdfDetector.detect(&Content::new(&hdf5_at(300, 1024))).is_none());
    }

    #[test]
    fn input_shorter_than_the_classic_magic_is_not_netcdf() {
        assert!(NetCdfDetector.detect(&Content::new(b"CDF")).is_none());
        assert!(NetCdfDetector.detect(&Content::new(b"")).is_none());
    }

    #[test]
    fn grib_and_sqlite_headers_are_not_netcdf() {
        assert!(NetCdfDetector.detect(&Content::new(b"GRIB\x00\x00\x00\x02")).is_none());
        let mut sqlite = b"SQLite format 3\0".to_vec();
        sqlite.resize(100, 0);
        assert!(NetCdfDetector.detect(&Content::new(&sqlite)).is_none());
    }
}
