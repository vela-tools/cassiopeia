use std::{
    fmt::Display,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Where one collector writes what it downloads: files named after it in the scratch folder of the
/// cycle attempt it collects for, a folder it shares with the attempt's other collectors.
///
/// The folder belongs to whatever runs the collectors. It exists before they start, and it is removed,
/// with every file in it, once the attempt ends. A collector writes files into it and never creates a
/// folder, so a collector still running once the folder is gone cannot bring it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchFiles {
    /// The scratch folder of the attempt, shared with every other collector of the attempt.
    folder: Arc<Path>,
    /// The collector's number among those that share the folder, which keeps their files apart.
    collector: u32,
}

impl ScratchFiles {
    /// The files of the collector numbered `collector` among those that share the scratch folder
    /// `folder`.
    #[must_use]
    pub const fn new(folder: Arc<Path>, collector: u32) -> ScratchFiles {
        ScratchFiles { folder, collector }
    }

    /// The file the collector writes its only download to, named with `extension`.
    pub(crate) fn only(&self, extension: impl Display) -> PathBuf {
        self.folder.join(format!("input-{}.{extension}", self.collector))
    }

    /// The file the collector writes the download numbered `index` of its several to, named with
    /// `extension`.
    pub(crate) fn numbered(&self, index: usize, extension: impl Display) -> PathBuf {
        self.folder.join(format!("input-{}-{index}.{extension}", self.collector))
    }
}

#[cfg(test)]
mod tests {
    use crate::scratch_files::ScratchFiles;
    use pretty_assertions::assert_eq;
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
    };

    fn files(collector: u32) -> ScratchFiles {
        ScratchFiles::new(Arc::from(Path::new("/scratch")), collector)
    }

    #[test]
    fn an_only_download_is_named_after_its_collector_and_its_extension() {
        assert_eq!(files(2).only("csv"), PathBuf::from("/scratch/input-2.csv"));
    }

    #[test]
    fn each_of_several_downloads_is_numbered_after_its_collector() {
        let files = files(2);

        assert_eq!(
            [files.numbered(0, "json"), files.numbered(1, "json")],
            [PathBuf::from("/scratch/input-2-0.json"), PathBuf::from("/scratch/input-2-1.json")]
        );
    }

    #[test]
    fn two_collectors_sharing_a_folder_never_name_the_same_file() {
        assert_ne!(files(0).only("csv"), files(1).only("csv"));
        assert_ne!(files(0).numbered(1, "csv"), files(1).numbered(0, "csv"));
    }
}
