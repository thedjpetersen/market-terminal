use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::features::spreadsheet::{SpreadsheetFileError, SpreadsheetFileStore};

const MAX_CSV_BYTES: u64 = 10 * 1024 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default)]
pub struct LocalSpreadsheetFiles;

impl SpreadsheetFileStore for LocalSpreadsheetFiles {
    fn read_csv(&self, location: &str) -> Result<String, SpreadsheetFileError> {
        let path = resolve_location(location)?;
        let metadata = fs::metadata(&path).map_err(|error| io_error("READ", &path, error))?;
        if metadata.len() > MAX_CSV_BYTES {
            return Err(SpreadsheetFileError::TooLarge);
        }
        let bytes = fs::read(&path).map_err(|error| io_error("READ", &path, error))?;
        String::from_utf8(bytes).map_err(|_| {
            SpreadsheetFileError::InvalidLocation(format!(
                "{} IS NOT A UTF-8 CSV FILE",
                path.display()
            ))
        })
    }

    fn write_csv(
        &self,
        location: &str,
        csv: &str,
        overwrite: bool,
    ) -> Result<(), SpreadsheetFileError> {
        if csv.len() as u64 > MAX_CSV_BYTES {
            return Err(SpreadsheetFileError::TooLarge);
        }
        let path = resolve_location(location)?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|error| io_error("CREATE DIRECTORY FOR", &path, error))?;
        if !overwrite {
            return write_new(&path, csv.as_bytes());
        }
        atomic_replace(&path, csv.as_bytes())
    }
}

fn resolve_location(location: &str) -> Result<PathBuf, SpreadsheetFileError> {
    let location = location.trim();
    if location.is_empty() {
        return Err(SpreadsheetFileError::InvalidLocation(
            "CSV PATH CANNOT BE EMPTY".to_owned(),
        ));
    }
    if location.contains('\0') {
        return Err(SpreadsheetFileError::InvalidLocation(
            "CSV PATH CONTAINS A NULL BYTE".to_owned(),
        ));
    }
    let path = PathBuf::from(location);
    if location == "~" {
        return env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| SpreadsheetFileError::InvalidLocation("HOME IS NOT SET".to_owned()));
    }
    if let Some(relative) = location.strip_prefix("~/") {
        return env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(relative))
            .ok_or_else(|| SpreadsheetFileError::InvalidLocation("HOME IS NOT SET".to_owned()));
    }
    Ok(path)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), SpreadsheetFileError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    configure_private_file(&mut options);
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(SpreadsheetFileError::AlreadyExists(
                path.display().to_string(),
            ));
        }
        Err(error) => return Err(io_error("WRITE", path, error)),
    };
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(io_error("WRITE", path, error));
    }
    Ok(())
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), SpreadsheetFileError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            SpreadsheetFileError::InvalidLocation("CSV PATH HAS NO FILE NAME".to_owned())
        })?;
    for _ in 0..16 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        configure_private_file(&mut options);
        let mut file = match options.open(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error("WRITE", &temporary, error)),
        };
        let result = (|| -> std::io::Result<()> {
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(io_error("REPLACE", path, error));
        }
        return Ok(());
    }
    Err(SpreadsheetFileError::Io(
        "COULD NOT RESERVE AN ATOMIC CSV EXPORT FILE".to_owned(),
    ))
}

#[cfg(unix)]
fn configure_private_file(options: &mut fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn configure_private_file(_options: &mut fs::OpenOptions) {}

fn io_error(operation: &str, path: &Path, error: std::io::Error) -> SpreadsheetFileError {
    SpreadsheetFileError::Io(format!("{operation} {} FAILED · {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_directory() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!(
            "market-terminal-spreadsheet-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn writes_reads_and_requires_explicit_overwrite() {
        let directory = temporary_directory();
        let path = directory.join("workbook.csv");
        let path_text = path.to_string_lossy();
        let files = LocalSpreadsheetFiles;

        files.write_csv(&path_text, "A,=1+1", false).unwrap();
        assert_eq!(files.read_csv(&path_text).unwrap(), "A,=1+1");
        assert!(matches!(
            files.write_csv(&path_text, "replacement", false),
            Err(SpreadsheetFileError::AlreadyExists(_))
        ));
        files.write_csv(&path_text, "replacement", true).unwrap();
        assert_eq!(files.read_csv(&path_text).unwrap(), "replacement");

        fs::remove_dir_all(directory).unwrap();
    }
}
