use bugreport::{
    CrateInfo,
    collector::{CollectionError, Collector},
    report::ReportEntry,
};
use cassiopeia_directories::locations::{config_file, log_dir, mappings_dir, schemas_dir};
use std::{fs, io::ErrorKind, path::Path};

/// A [`Collector`] reporting the filesystem locations a run reads from and writes to, each annotated
/// with a coarse state: a file by whether it is present, a directory by how many entries it holds.
///
/// It never reports a file's contents or the names of individual entries. The configuration file can
/// hold broker credentials and the logs can hold ingested data, and a full schema or mapping listing
/// is long and rarely the point. The count alone answers what a report needs: are the Smart Data
/// Model schemas downloaded, is a mapping present, without naming anything.
pub struct RunLocations;

/// Renders a file location as `label: path (present|absent)`. Existence is probed without opening
/// the file, so no contents are read.
fn describe_file(label: &str, path: &Path) -> ReportEntry {
    let presence = if path.exists() { "present" } else { "absent" };
    ReportEntry::Text(format!("{label}: {} ({presence})", path.display()))
}

/// Renders a directory location as `label: path (N entries|empty|absent|unreadable)`. Only the
/// top-level entries are counted and their names are never emitted.
fn describe_directory(label: &str, path: &Path) -> ReportEntry {
    let state = match fs::read_dir(path) {
        Ok(entries) => match entries.count() {
            0 => "empty".to_owned(),
            count => format!("{count} entries"),
        },
        Err(error) if error.kind() == ErrorKind::NotFound => "absent".to_owned(),
        Err(_) => "unreadable".to_owned(),
    };
    ReportEntry::Text(format!("{label}: {} ({state})", path.display()))
}

impl Collector for RunLocations {
    fn description(&self) -> &'static str {
        "Run locations"
    }

    fn collect(&mut self, _crate_info: &CrateInfo<'_>) -> Result<ReportEntry, CollectionError> {
        Ok(ReportEntry::List(vec![
            describe_file("Configuration file", &config_file()),
            describe_directory("Downloaded schemas", &schemas_dir()),
            describe_directory("Mapping documents", &mappings_dir()),
            describe_directory("Log directory", &log_dir()),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use crate::bug_report::run_locations::{describe_directory, describe_file};
    use bugreport::report::ReportEntry;
    use std::{env, path::PathBuf};

    #[test]
    fn an_existing_file_path_is_reported_present() {
        let here = env::current_dir().expect("the test has a working directory");
        let ReportEntry::Text(line) = describe_file("Working directory", &here) else {
            panic!("describe_file renders a text entry");
        };

        assert!(line.contains("Working directory: "));
        assert!(line.ends_with("(present)"));
    }

    #[test]
    fn a_missing_file_path_is_reported_absent() {
        let missing = PathBuf::from("/cassiopeia/does/not/exist/anywhere");
        let ReportEntry::Text(line) = describe_file("Configuration file", &missing) else {
            panic!("describe_file renders a text entry");
        };

        assert!(line.ends_with("(absent)"));
    }

    #[test]
    fn a_populated_directory_is_reported_by_entry_count() {
        let here = env::current_dir().expect("the test has a working directory");
        let ReportEntry::Text(line) = describe_directory("Downloaded schemas", &here) else {
            panic!("describe_directory renders a text entry");
        };

        assert!(line.contains("entries)"));
    }

    #[test]
    fn a_missing_directory_is_reported_absent() {
        let missing = PathBuf::from("/cassiopeia/does/not/exist/anywhere");
        let ReportEntry::Text(line) = describe_directory("Downloaded schemas", &missing) else {
            panic!("describe_directory renders a text entry");
        };

        assert!(line.ends_with("(absent)"));
    }
}
