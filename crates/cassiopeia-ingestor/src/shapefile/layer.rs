use crate::shapefile::error::ShapefileIngestError;
use cassiopeia_common::collection::CollectionName;
use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};
use temp_dir::TempDir;
use zip::ZipArchive;

/// The zip local-file-header magic (`PK\x03\x04`) that opens a shapefile bundle.
const ZIP_MAGIC: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
/// The four-octet big-endian file code (9994) a `.shp` main file opens with.
const SHP_FILE_CODE: [u8; 4] = [0x00, 0x00, 0x27, 0x0A];

/// The physical carrier of a shapefile input, chosen from the collected file's leading bytes rather
/// than its declared extension (a remote `.zip` is downloaded under a `.shp` name, so the content, not
/// the name, is authoritative).
#[derive(Debug)]
pub enum ShapefileSource {
    /// A bare local `.shp` main file; the `shapefile` reader resolves its `.dbf`/`.shx`/`.prj`/`.cpg`
    /// companions from the same directory.
    Bare(PathBuf),
    /// A `.zip` bundle whose `.shp` layers and their companions are extracted before reading.
    Zip(PathBuf),
}

/// One discovered shapefile layer: its `.shp` path and the collection it routes to.
pub struct Layer {
    /// The `.shp` main file to open; its companions sit beside it.
    pub shp_path: PathBuf,
    /// The collection this layer's records carry: `None` for a lone layer, the layer basename when a
    /// bundle holds several, mirroring how a folder-less KML placemark or a single-sheet workbook
    /// degrades to no collection.
    pub collection: Option<CollectionName>,
}

/// The layers discovered from a source, plus the temporary directory holding any extracted zip members.
pub struct DiscoveredLayers {
    /// Kept alive for the reader's lifetime: its `Drop` deletes the extracted members, so it must
    /// outlive every layer read. `None` for a bare `.shp`, whose files are already on disk.
    pub extraction: Option<TempDir>,
    /// The layers to read, in deterministic order.
    pub layers: Vec<Layer>,
}

impl ShapefileSource {
    /// Chooses the carrier from the leading bytes of the collected file.
    ///
    /// # Errors
    ///
    /// Returns [`ShapefileIngestError::Io`] when the file cannot be opened or read, or
    /// [`ShapefileIngestError::UnknownCarrier`] when the leading bytes are neither the zip magic nor the
    /// `.shp` file code.
    pub fn from_path(path: PathBuf) -> Result<ShapefileSource, ShapefileIngestError> {
        let mut file = File::open(&path).map_err(|source| ShapefileIngestError::Io { source, path: path.clone() })?;
        let mut header = [0u8; 4];
        let mut filled = 0;
        while filled < header.len() {
            let read = file
                .read(&mut header[filled..])
                .map_err(|source| ShapefileIngestError::Io { source, path: path.clone() })?;
            if read == 0 {
                break;
            }
            filled += read;
        }

        let header = &header[..filled];
        if header.starts_with(&ZIP_MAGIC) {
            Ok(ShapefileSource::Zip(path))
        } else if header.starts_with(&SHP_FILE_CODE) {
            Ok(ShapefileSource::Bare(path))
        } else {
            Err(ShapefileIngestError::UnknownCarrier)
        }
    }

    /// Discovers the layers this source carries, extracting a zip bundle to a temporary directory first.
    ///
    /// # Errors
    ///
    /// Returns [`ShapefileIngestError`] when the bundle cannot be opened or extracted, or when it holds
    /// no `.shp` layer.
    pub fn discover(&self) -> Result<DiscoveredLayers, ShapefileIngestError> {
        match self {
            ShapefileSource::Bare(shp_path) => Ok(DiscoveredLayers {
                extraction: None,
                layers: vec![Layer {
                    shp_path: shp_path.clone(),
                    collection: None,
                }],
            }),
            ShapefileSource::Zip(archive_path) => discover_zip(archive_path),
        }
    }
}

/// Extracts a zip bundle to a temporary directory and discovers its `.shp` layers.
fn discover_zip(archive_path: &Path) -> Result<DiscoveredLayers, ShapefileIngestError> {
    let file = File::open(archive_path).map_err(|source| ShapefileIngestError::Io {
        source,
        path: archive_path.to_path_buf(),
    })?;
    let mut archive = ZipArchive::new(file)?;
    let extraction = TempDir::new().map_err(|source| ShapefileIngestError::Io {
        source,
        path: archive_path.to_path_buf(),
    })?;
    let directory = extraction.path();

    let mut shp_paths: Vec<PathBuf> = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        // Flatten to the basename: a shapefile's siblings must be co-located under one stem, so nested
        // members are placed side by side rather than in mirrored subdirectories.
        let Some(name) = entry.enclosed_name().and_then(|path| path.file_name().map(ToOwned::to_owned)) else {
            continue;
        };
        let out_path = directory.join(&name);
        let mut out = File::create(&out_path).map_err(|source| ShapefileIngestError::Io {
            source,
            path: out_path.clone(),
        })?;
        io::copy(&mut entry, &mut out).map_err(|source| ShapefileIngestError::Io {
            source,
            path: out_path.clone(),
        })?;

        if out_path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("shp")) {
            shp_paths.push(out_path);
        }
    }

    if shp_paths.is_empty() {
        return Err(ShapefileIngestError::NoLayers);
    }

    shp_paths.sort();
    let single_layer = shp_paths.len() == 1;
    let layers = shp_paths
        .into_iter()
        .map(|shp_path| {
            let collection = if single_layer {
                None
            } else {
                shp_path.file_stem().and_then(|stem| stem.to_str()).map(CollectionName::from)
            };
            Layer { shp_path, collection }
        })
        .collect();

    Ok(DiscoveredLayers {
        extraction: Some(extraction),
        layers,
    })
}
