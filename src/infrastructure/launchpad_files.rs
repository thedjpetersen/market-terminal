use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::features::launchpad::{
    LaunchpadFileError, LaunchpadFileStore, MAX_LAUNCHPAD_DOCUMENT_BYTES,
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default)]
pub struct LocalLaunchpadFiles;

impl LaunchpadFileStore for LocalLaunchpadFiles {
    fn read_document(&self, location: &str) -> Result<String, LaunchpadFileError> {
        let path = resolve_location(location)?;
        let metadata = fs::metadata(&path).map_err(|error| io_error("READ", &path, error))?;
        if metadata.len() > MAX_LAUNCHPAD_DOCUMENT_BYTES as u64 {
            return Err(LaunchpadFileError::TooLarge);
        }
        let bytes = fs::read(&path).map_err(|error| io_error("READ", &path, error))?;
        String::from_utf8(bytes).map_err(|_| {
            LaunchpadFileError::InvalidLocation(format!(
                "{} IS NOT A UTF-8 JSON FILE",
                path.display()
            ))
        })
    }

    fn write_document(
        &self,
        location: &str,
        document: &str,
        overwrite: bool,
    ) -> Result<(), LaunchpadFileError> {
        if document.len() > MAX_LAUNCHPAD_DOCUMENT_BYTES {
            return Err(LaunchpadFileError::TooLarge);
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

fn resolve_location(location: &str) -> Result<PathBuf, LaunchpadFileError> {
    let location = location.trim();
    if location.is_empty() || location.contains('\0') {
        return Err(LaunchpadFileError::InvalidLocation(
            "JSON PATH IS EMPTY OR CONTAINS A NULL BYTE".to_owned(),
        ));
    }
    if location == "~" {
        return env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| LaunchpadFileError::InvalidLocation("HOME IS NOT SET".to_owned()));
    }
    if let Some(relative) = location.strip_prefix("~/") {
        return env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(relative))
            .ok_or_else(|| LaunchpadFileError::InvalidLocation("HOME IS NOT SET".to_owned()));
    }
    Ok(PathBuf::from(location))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), LaunchpadFileError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    configure_private_file(&mut options);
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(LaunchpadFileError::AlreadyExists(
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

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), LaunchpadFileError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            LaunchpadFileError::InvalidLocation("JSON PATH HAS NO FILE NAME".to_owned())
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
    Err(LaunchpadFileError::Io(
        "COULD NOT RESERVE AN ATOMIC JSON EXPORT FILE".to_owned(),
    ))
}

#[cfg(unix)]
fn configure_private_file(options: &mut fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn configure_private_file(_options: &mut fs::OpenOptions) {}

fn io_error(operation: &str, path: &Path, error: std::io::Error) -> LaunchpadFileError {
    LaunchpadFileError::Io(format!("{operation} {} FAILED · {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn portable_files_are_private_and_require_explicit_overwrite() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = env::temp_dir().join(format!(
            "market-terminal-launchpad-{}-{unique}",
            std::process::id()
        ));
        let path = directory.join("launchpad.json");
        let location = path.to_string_lossy();
        let files = LocalLaunchpadFiles;
        files.write_document(&location, "{}", false).unwrap();
        assert!(matches!(
            files.write_document(&location, "[]", false),
            Err(LaunchpadFileError::AlreadyExists(_))
        ));
        files.write_document(&location, "[]", true).unwrap();
        assert_eq!(files.read_document(&location).unwrap(), "[]");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(directory).unwrap();
    }
}
