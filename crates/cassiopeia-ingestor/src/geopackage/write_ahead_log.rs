use crate::geopackage::error::GeoPackageIngestError;
use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

/// The suffix SQLite appends to the full path of a database to name its write-ahead log.
const WRITE_AHEAD_LOG_SUFFIX: &str = "-wal";

/// Resolves the path a `GeoPackage` is handed to SQLite by, refusing it when SQLite would read a
/// write-ahead log with it.
///
/// SQLite before 3.54.0 returns uninitialised page-cache memory as row data when it reads a frame from a
/// `-wal` file whose page size differs from the database's (RUSTSEC-2026-0459, CVE-2026-119375), and
/// the SQLite the `geopackage` crate bundles predates that fix. SQLite opens the log of a non-empty
/// database whenever a file named by the database's full path plus `-wal` exists, whatever journal mode
/// the database header records, so the existence of that file is the exact condition refused here.
///
/// The returned path is the canonical form of `database`, and it is the path SQLite must be given:
///
/// - It is absolute, so SQLite, which the bundled build compiles with `SQLITE_USE_URI`, cannot read it
///   as a `file:` URI naming another file or VFS.
/// - Every symbolic link in it is resolved, so the full path SQLite names the log by is the one checked
///   here on every platform. Handed an unresolved path, the Unix VFS would name the log beside the link's
///   target and the Windows VFS beside the link.
///
/// # Errors
///
/// Returns [`GeoPackageIngestError::WriteAheadLog`] when a log exists beside the resolved path, or
/// [`GeoPackageIngestError::WriteAheadLogUnverified`] when whether one exists cannot be determined, such
/// as when `database` cannot be resolved.
pub fn canonical_path_without_write_ahead_log(database: &Path) -> Result<PathBuf, GeoPackageIngestError> {
    let unverified = |source: io::Error| GeoPackageIngestError::WriteAheadLogUnverified {
        database: database.to_path_buf(),
        source,
    };
    let canonical = fs::canonicalize(database).map_err(unverified)?;
    let log = write_ahead_log_path(&canonical);
    match log.try_exists() {
        Ok(false) => Ok(canonical),
        Ok(true) => Err(GeoPackageIngestError::WriteAheadLog { log }),
        Err(source) => Err(unverified(source)),
    }
}

/// The path SQLite names the write-ahead log of `database` by: the database path with `-wal` appended.
fn write_ahead_log_path(database: &Path) -> PathBuf {
    let mut log = database.as_os_str().to_owned();
    log.push(WRITE_AHEAD_LOG_SUFFIX);
    PathBuf::from(log)
}

#[cfg(test)]
mod tests {
    use crate::geopackage::{error::GeoPackageIngestError, write_ahead_log::canonical_path_without_write_ahead_log};
    use pretty_assertions::assert_eq;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::{
        fs,
        path::{Path, PathBuf},
    };
    use temp_dir::TempDir;

    /// Writes an empty file at `path`; the guard looks only at which files exist, never at their bytes.
    fn touch(path: &Path) {
        fs::write(path, b"").unwrap();
    }

    #[test]
    fn a_geopackage_without_a_write_ahead_log_beside_it_is_opened_by_its_absolute_canonical_path() {
        let dir = TempDir::new().unwrap();
        let database = dir.path().join("stations.gpkg");
        touch(&database);

        let opened = canonical_path_without_write_ahead_log(&database).unwrap();
        assert!(opened.is_absolute());
        assert_eq!(opened, fs::canonicalize(&database).unwrap());
    }

    #[test]
    fn a_rollback_journal_or_a_differently_named_log_beside_the_geopackage_is_not_refused() {
        let dir = TempDir::new().unwrap();
        let database = dir.path().join("stations.gpkg");
        touch(&database);
        touch(&dir.path().join("stations.gpkg-journal"));
        touch(&dir.path().join("stations.wal"));

        assert!(canonical_path_without_write_ahead_log(&database).is_ok());
    }

    #[test]
    fn a_write_ahead_log_beside_the_geopackage_is_refused_and_named() {
        let dir = TempDir::new().unwrap();
        let database = dir.path().join("stations.gpkg");
        touch(&database);
        touch(&dir.path().join("stations.gpkg-wal"));

        let error = canonical_path_without_write_ahead_log(&database).unwrap_err();
        let expected = fs::canonicalize(dir.path()).unwrap().join("stations.gpkg-wal");
        assert!(matches!(&error, GeoPackageIngestError::WriteAheadLog { log } if *log == expected));
    }

    #[cfg(unix)]
    #[test]
    fn a_write_ahead_log_beside_the_target_of_a_symbolic_link_is_refused() {
        let dir = TempDir::new().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        touch(&real.join("stations.gpkg"));
        touch(&real.join("stations.gpkg-wal"));
        let link = dir.path().join("link.gpkg");
        symlink(real.join("stations.gpkg"), &link).unwrap();

        let error = canonical_path_without_write_ahead_log(&link).unwrap_err();
        let expected = fs::canonicalize(&real).unwrap().join("stations.gpkg-wal");
        assert!(matches!(&error, GeoPackageIngestError::WriteAheadLog { log } if *log == expected));
    }

    #[cfg(unix)]
    #[test]
    fn a_log_beside_a_symbolic_link_itself_is_not_refused_because_sqlite_is_handed_the_link_target() {
        let dir = TempDir::new().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        touch(&real.join("stations.gpkg"));
        let link = dir.path().join("link.gpkg");
        symlink(real.join("stations.gpkg"), &link).unwrap();
        touch(&dir.path().join("link.gpkg-wal"));

        let opened = canonical_path_without_write_ahead_log(&link).unwrap();
        assert_eq!(opened, fs::canonicalize(real.join("stations.gpkg")).unwrap());
    }

    #[test]
    fn a_geopackage_that_cannot_be_resolved_is_refused_as_unverified() {
        let dir = TempDir::new().unwrap();
        let database = dir.path().join("missing.gpkg");

        let error = canonical_path_without_write_ahead_log(&database).unwrap_err();
        assert!(matches!(&error, GeoPackageIngestError::WriteAheadLogUnverified { database: named, .. } if *named == database));
    }

    #[test]
    fn the_refusal_names_the_log_and_the_advisory() {
        let error = GeoPackageIngestError::WriteAheadLog {
            log: PathBuf::from("/data/stations.gpkg-wal"),
        };

        assert_eq!(
            error.to_string(),
            "the GeoPackage has a write-ahead log beside it at '/data/stations.gpkg-wal', and Cassiopeia does not read a GeoPackage together with its log (RUSTSEC-2026-0459); close the program that holds the GeoPackage open so the log is merged back into it, then run again"
        );
    }
}
