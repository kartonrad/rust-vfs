//! Virtual file system abstraction
//!
//! The virtual file system abstraction generalizes over file systems and allow using
//! different filesystem implementations (i.e. an in memory implementation for unit tests)
//!
//! A virtual filesystem consists of three basic types
//!
//!  * **Paths** - locations in the filesystem
//!  * **File** - actual file contents (think inodes)
//!  * **Metadata** - metadata information about paths
//!
//!
//! This crate currently has the following implementations:
//!
//!  * **PhysicalFS** - the actual filesystem of the underlying OS
//!  * **MemoryFS** - an ephemeral in-memory implementation (intended for unit tests)

#[cfg(test)]
#[macro_use]
pub mod test_macros;

pub mod memory;
pub mod physical;

use std::fmt::{Debug, Display};
use std::io::{Read, Seek, Write};
use std::sync::Arc;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum VfsError {
    #[error("data store disconnected")]
    IoError(#[from] std::io::Error),
    #[error("the file or directory `{path}` could not be found")]
    FileNotFound { path: String },
    #[error("other VFS error: {message}")]
    Other { message: String },
    #[error("{context}, cause: {cause}")]
    WithContext {
        context: String,
        #[source]
        cause: Box<VfsError>,
    },
}

pub type Result<T> = std::result::Result<T, VfsError>;

pub trait ResultExt<T> {
    fn with_context<C, F>(self, f: F) -> Result<T>
    where
        C: Display + Send + Sync + 'static,
        F: FnOnce() -> C;
}

impl<T> ResultExt<T> for Result<T> {
    fn with_context<C, F>(self, context: F) -> Result<T>
    where
        C: Display + Send + Sync + 'static,
        F: FnOnce() -> C,
    {
        self.map_err(|error| VfsError::WithContext {
            context: context().to_string(),
            cause: Box::new(error),
        })
    }
}

pub trait SeekAndRead: Seek + Read {}

impl<T> SeekAndRead for T where T: Seek + Read {}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VFileType {
    File,
    Directory,
}

#[derive(Debug)]
pub struct VMetadata {
    pub file_type: VFileType,
    pub len: u64,
}

pub trait VFS: Debug + Sync + Send {
    fn read_dir(&self, path: &str) -> Result<Box<dyn Iterator<Item = String>>>;
    fn create_dir(&self, path: &str) -> Result<()>;
    fn open_file(&self, path: &str) -> Result<Box<dyn SeekAndRead>>;
    fn create_file(&self, path: &str) -> Result<Box<dyn Write>>;
    fn append_file(&self, path: &str) -> Result<Box<dyn Write>>;
    fn metadata(&self, path: &str) -> Result<VMetadata>;
    fn exists(&self, path: &str) -> bool;
    fn remove_file(&self, path: &str) -> Result<()>;
    fn remove_dir(&self, path: &str) -> Result<()>;
}

#[derive(Debug)]
pub struct FileSystem {
    vfs: Box<dyn VFS>,
}

#[derive(Debug)]
pub struct VPath {
    path: String,
    fs: Arc<FileSystem>,
}

impl VPath {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn join(&self, path: &str) -> Self {
        VPath {
            path: format!("{}/{}", self.path, path),
            fs: self.fs.clone(),
        }
    }

    pub fn read_dir(&self) -> Result<Box<dyn Iterator<Item = VPath>>> {
        let parent = self.path.clone();
        let fs = self.fs.clone();
        Ok(Box::new(
            self.fs
                .vfs
                .read_dir(&self.path)
                .with_context(|| format!("Could not read directory '{}'", &self.path))?
                .map(move |path| VPath {
                    path: format!("{}/{}", parent, path),
                    fs: fs.clone(),
                }),
        ))
    }

    pub fn create_dir(&self) -> Result<()> {
        self.fs
            .vfs
            .create_dir(&self.path)
            .with_context(|| format!("Could not create directory '{}'", &self.path))
    }

    pub fn create_dir_all(&self) -> Result<()> {
        let mut pos = 1;
        let path = &self.path;
        loop {
            // Iterate over path segments
            let end = path[pos..]
                .find('/')
                .map(|it| it + pos)
                .unwrap_or_else(|| path.len());
            let directory = &path[..end];
            if !self.fs.vfs.exists(directory) {
                self.fs.vfs.create_dir(directory)?;
            }
            if end == path.len() {
                break;
            }
            pos = end + 1;
        }
        Ok(())
    }

    pub fn open_file(&self) -> Result<Box<dyn SeekAndRead>> {
        self.fs
            .vfs
            .open_file(&self.path)
            .with_context(|| format!("Could not open file '{}'", &self.path))
    }
    pub fn create_file(&self) -> Result<Box<dyn Write>> {
        self.fs
            .vfs
            .create_file(&self.path)
            .with_context(|| format!("Could not create file '{}'", &self.path))
    }
    pub fn append_file(&self) -> Result<Box<dyn Write>> {
        self.fs
            .vfs
            .append_file(&self.path)
            .with_context(|| format!("Could not open file '{}' for appending", &self.path))
    }
    pub fn remove_file(&self) -> Result<()> {
        self.fs
            .vfs
            .remove_file(&self.path)
            .with_context(|| format!("Could not remove file '{}'", &self.path))
    }

    pub fn remove_dir(&self) -> Result<()> {
        self.fs
            .vfs
            .remove_dir(&self.path)
            .with_context(|| format!("Could not remove directory '{}'", &self.path))
    }

    pub fn remove_dir_all(&self) -> Result<()> {
        if !self.exists() {
            return Ok(());
        }
        for child in self.read_dir()? {
            let metadata = child.metadata()?;
            match metadata.file_type {
                VFileType::File => child.remove_file()?,
                VFileType::Directory => child.remove_dir_all()?,
            }
        }
        self.remove_dir()?;
        Ok(())
    }

    pub fn metadata(&self) -> Result<VMetadata> {
        self.fs
            .vfs
            .metadata(&self.path)
            .with_context(|| format!("Could get metadata for '{}'", &self.path))
    }

    pub fn exists(&self) -> bool {
        self.fs.vfs.exists(&self.path)
    }
    pub fn create<T: VFS + 'static>(vfs: T) -> Result<Self> {
        Ok(VPath {
            path: "".to_string(),
            fs: Arc::new(FileSystem { vfs: Box::new(vfs) }),
        })
    }
}

/*

#![allow(unused_imports)]
#![allow(unused_variables)]

#[macro_use]
mod macros {
    use std::io::{Result, Error, ErrorKind};
    use std;

    fn to_io_error<E: std::error::Error>(error: E) -> Error {
        Error::new(ErrorKind::Other, error.description())
    }

    pub fn to_io_result<T, E: std::error::Error>(result: std::result::Result<T, E>) -> Result<T> {
        match result {
            Ok(result) => Ok(result),
            Err(error) => Err(to_io_error(error)),
        }
    }

    macro_rules! ctry {
    ($result:expr) => (try!($crate::macros::to_io_result($result)));
    }


}



pub mod physical;
pub use physical::PhysicalFS;

pub mod altroot;
pub use altroot::AltrootFS;

pub mod memory;
pub use memory::MemoryFS;

pub mod util;

use std::path::{Path, PathBuf};
use std::convert::AsRef;

use std::fmt::Debug;
use std::io::{Read, Write, Seek, Result};
use std::borrow::Cow;

/// A abstract path to a location in a filesystem
pub trait VPath: Debug + std::marker::Send + std::marker::Sync {
    /// Open the file at this path with the given options
    fn open_with_options(&self, openOptions: &OpenOptions) -> Result<Box<VFile>>;
    /// Open the file at this path for reading
    fn open(&self) -> Result<Box<VFile>> {
        self.open_with_options(OpenOptions::new().read(true))
    }
    /// Open the file at this path for writing, truncating it if it exists already
    fn create(&self) -> Result<Box<VFile>> {
        self.open_with_options(OpenOptions::new().write(true).create(true).truncate(true))
    }
    /// Open the file at this path for appending, creating it if necessary
    fn append(&self) -> Result<Box<VFile>> {
        self.open_with_options(OpenOptions::new().write(true).create(true).append(true))
    }
    /// Create a directory at the location by this path
    fn mkdir(&self) -> Result<()>;

    /// Remove a file
    fn rm(&self) -> Result<()>;

    /// Remove a file or directory and all its contents
    fn rmrf(&self) -> Result<()>;


    /// The file name of this path
    fn file_name(&self) -> Option<String>;

    /// The extension of this filename
    fn extension(&self) -> Option<String>;

    /// append a segment to this path
    fn resolve(&self, path: &String) -> Box<VPath>;

    /// Get the parent path
    fn parent(&self) -> Option<Box<VPath>>;

    /// Check if the file existst
    fn exists(&self) -> bool;

    /// Get the file's metadata
    fn metadata(&self) -> Result<Box<VMetadata>>;

    /// Retrieve the path entries in this path
    fn read_dir(&self) -> Result<Box<Iterator<Item = Result<Box<VPath>>>>>;

    /// Retrieve a string representation
    fn to_string(&self) -> Cow<str>;

    /// Retrieve a standard PathBuf, if available (usually only for PhysicalFS)
    fn to_path_buf(&self) -> Option<PathBuf>;

    fn box_clone(&self) -> Box<VPath>;
}

impl Clone for Box<VPath> {
    fn clone(&self) -> Box<VPath> {
        self.box_clone()
    }
}


/// Resolve the path relative to the given base returning a new path
pub fn resolve<S: Into<String>>(base: &VPath, path: S) -> Box<VPath> {
    base.resolve(&path.into())
}

/// An abstract file object
pub trait VFile: Read + Write + Seek + Debug {}

impl<T> VFile for T where T: Read + Write + Seek + Debug {}

/// File metadata abstraction
pub trait VMetadata {
    /// Returns true iff this path is a directory
    fn is_dir(&self) -> bool;
    /// Returns true iff this path is a file
    fn is_file(&self) -> bool;
    /// Returns the length of the file at this path
    fn len(&self) -> u64;
}

/// An abstract virtual file system
pub trait VFS {
    /// The type of file objects
    type PATH: VPath;
    /// The type of path objects
    type FILE: VFile;
    /// The type of metadata objects
    type METADATA: VMetadata;

    /// Create a new path within this filesystem
    fn path<T: Into<String>>(&self, path: T) -> Self::PATH;
}


/// Options for opening files
#[derive(Debug, Default)]
pub struct OpenOptions {
    pub read: bool,
    pub write: bool,
    pub create: bool,
    pub append: bool,
    pub truncate: bool,
}

impl OpenOptions {
    /// Create a new instance
    pub fn new() -> OpenOptions {
        Default::default()
    }

    /// Open for reading
    pub fn read(&mut self, read: bool) -> &mut OpenOptions {
        self.read = read;
        self
    }

    /// Open for writing
    pub fn write(&mut self, write: bool) -> &mut OpenOptions {
        self.write = write;
        self
    }

    /// Create the file if it does not exist yet
    pub fn create(&mut self, create: bool) -> &mut OpenOptions {
        self.create = create;
        self
    }

    /// Append at the end of the file
    pub fn append(&mut self, append: bool) -> &mut OpenOptions {
        self.append = append;
        self
    }

    /// Truncate the file to 0 bytes after opening
    pub fn truncate(&mut self, truncate: bool) -> &mut OpenOptions {
        self.truncate = truncate;
        self
    }
}
*/
