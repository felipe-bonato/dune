use std::{fs, io, os::unix::fs::PermissionsExt, path, time};

pub static INVALID_FILE: &str = "<INVALID>";

pub struct FileInfo {
    name: String,
    path_abs: path::PathBuf,
    is_dir: bool,
    permissions: fs::Permissions,
    last_modified: time::SystemTime,
    size_kib: u64,
}

impl FileInfo {
    pub fn path_str(&self) -> &str {
        self.path_abs.to_str().unwrap_or(INVALID_FILE)
    }

    pub fn path(&self) -> &path::Path {
        &self.path_abs
    }

    pub fn mode(&self) -> u32 {
        self.permissions.mode()
    }

    pub fn is_dir(&self) -> bool {
        self.is_dir
    }

    pub fn is_read_only(&self) -> bool {
        self.permissions.readonly()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn last_modified(&self) -> chrono::DateTime<chrono::Local> {
        // This is the only place where we use chrono.
        // Is this really needed?
        self.last_modified.into()
    }

    /// Returns a pretty printed (with unit) size.
    /// Eg.: 10KiB, 1.0MiB
    pub fn pretty_size(&self) -> String {
        if self.size_kib > 1024 * 1024 * 1024 * 1024 * 1024 {
            format!("{s:3} PiB", s = self.size_kib / 1024 * 1024 * 1024 * 1024 * 1024)
        } else if self.size_kib > 1024 * 1024 * 1024 * 1024 {
            format!("{s:3} TiB", s = self.size_kib / 1024 * 1024 * 1024 * 1024)
        } else if self.size_kib > 1024 * 1024 * 1024 {
            format!("{s:3} GiB", s = self.size_kib / 1024 * 1024 * 1024)
        } else if self.size_kib > 1024 * 1024 {
            format!("{s:3} MiB", s = self.size_kib / 1024 * 1024)
        } else if self.size_kib > 1024 {
            format!("{s:3} KiB", s = self.size_kib / 1024)
        } else {
            format!("{s:3} B", s = self.size_kib)
        }
    }
}

impl TryFrom<path::PathBuf> for FileInfo {
    type Error = io::Error;

    fn try_from(path: path::PathBuf) -> Result<Self, Self::Error> {
        let metadata = fs::metadata(&path)?;
        Ok(FileInfo {
            is_dir: metadata.is_dir(),
            name: path
                .file_name()
                .unwrap_or_default() // TODO: Handle invalid files better
                .to_str()
                .unwrap_or(INVALID_FILE)
                .to_owned(),
            permissions: metadata.permissions(),
            path_abs: path,
            last_modified: metadata.modified()?, // TODO: Handle platforms where there is no modified time saved
            size_kib: metadata.len(),
        })
    }
}

impl TryFrom<fs::DirEntry> for FileInfo {
    type Error = io::Error;

    fn try_from(value: fs::DirEntry) -> Result<Self, Self::Error> {
        let metadata = value.metadata()?;
        Ok(FileInfo {
            name: value
                .file_name()
                .to_str()
                .unwrap_or(INVALID_FILE)
                .to_owned(),
            path_abs: value.path(),
            is_dir: metadata.is_dir(),
            permissions: metadata.permissions(),
            last_modified: metadata.modified()?,
            size_kib: metadata.len(),
        })
    }
}