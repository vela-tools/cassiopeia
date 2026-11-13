use crate::error::CollectorError;
use std::{
    io,
    path::{Path, PathBuf},
};

/// The failure of writing the snapshot at `path`.
pub(crate) fn unwritable(path: &Path) -> impl Fn(io::Error) -> CollectorError + '_ {
    move |source| CollectorError::Io {
        source,
        path: PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::CollectorError, sensorthings::snapshot_failure::unwritable};
    use std::{
        io::{self, ErrorKind},
        path::Path,
    };

    #[test]
    fn a_write_failure_names_the_snapshot() {
        let failure = unwritable(Path::new("/tmp/snapshot.jsonl"))(io::Error::from(ErrorKind::StorageFull));

        assert!(
            matches!(&failure, CollectorError::Io { path, .. } if path == Path::new("/tmp/snapshot.jsonl")),
            "{failure:?}"
        );
    }
}
