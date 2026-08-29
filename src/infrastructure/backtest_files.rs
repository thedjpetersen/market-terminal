use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::features::backtesting::{
    BacktestArtifactError, BacktestArtifactFileStore, MAX_BACKTEST_EXPORT_BYTES,
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default)]
pub struct LocalBacktestArtifactFiles;

impl BacktestArtifactFileStore for LocalBacktestArtifactFiles {
    fn write_artifact(
        &self,
        location: &str,
        document: &str,
        overwrite: bool,
    ) -> Result<(), BacktestArtifactError> {
        if document.len() > MAX_BACKTEST_EXPORT_BYTES {
            return Err(BacktestArtifactError::TooLarge);
        }
        let path = resolve_location(location)?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|error| io_error("CREATE DIRECTORY FOR", &path, error))?;
        if overwrite {
            atomic_replace(&path, document.as_bytes())
        } else {
            write_new(&path, document.as_bytes())
        }
    }
}

fn resolve_location(location: &str) -> Result<PathBuf, BacktestArtifactError> {
    let location = location.trim();
    if location.is_empty() || location.contains('\0') {
        return Err(BacktestArtifactError::InvalidLocation(
            "JSON path is empty or contains a null byte".to_owned(),
        ));
    }
    if location == "~" {
        return env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| BacktestArtifactError::InvalidLocation("HOME is not set".to_owned()));
    }
    if let Some(relative) = location.strip_prefix("~/") {
        return env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(relative))
            .ok_or_else(|| BacktestArtifactError::InvalidLocation("HOME is not set".to_owned()));
    }
    Ok(PathBuf::from(location))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), BacktestArtifactError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    configure_private_file(&mut options);
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(BacktestArtifactError::AlreadyExists(
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

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), BacktestArtifactError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            BacktestArtifactError::InvalidLocation("path has no file name".to_owned())
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
    Err(BacktestArtifactError::Io(
        "could not reserve an atomic JSON export file".to_owned(),
    ))
}

#[cfg(unix)]
fn configure_private_file(options: &mut fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn configure_private_file(_options: &mut fs::OpenOptions) {}

fn io_error(operation: &str, path: &Path, error: std::io::Error) -> BacktestArtifactError {
    BacktestArtifactError::Io(format!("{operation} {} failed: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn exports_are_private_and_require_explicit_overwrite() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = env::temp_dir().join(format!(
            "market-terminal-backtest-export-{}-{unique}",
            std::process::id()
        ));
        let path = directory.join("run.json");
        let location = path.to_string_lossy();
        let files = LocalBacktestArtifactFiles;
        files.write_artifact(&location, "{}", false).unwrap();
        assert!(matches!(
            files.write_artifact(&location, "[]", false),
            Err(BacktestArtifactError::AlreadyExists(_))
        ));
        files.write_artifact(&location, "[]", true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "[]");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = fs::remove_dir_all(directory);
    }
}
