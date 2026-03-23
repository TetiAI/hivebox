//! Sandbox image management.
//!
//! Images are read-only squashfs files containing a complete Debian Linux rootfs.
//! They serve as the base layer in the overlayfs stack — each sandbox gets the
//! image as its read-only lower layer plus a writable tmpfs upper layer.
//!
//! # Image storage layout
//!
//! ```text
//! /var/lib/hivebox/images/
//! ├── base.squashfs     (~30 MB)  — minimal Debian with coreutils + apt
//! ├── python.squashfs   (~80 MB)  — Debian + Python 3 + pip
//! ├── node.squashfs     (~75 MB)  — Debian + Node.js + npm
//! └── ml.squashfs       (~200 MB) — Debian + Python + numpy/scipy/sklearn
//! ```
//!
//! Images are built by the scripts in `images/` and can be rebuilt at any time.
//! The squashfs format provides excellent compression and fast random-access reads.

pub mod builder;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use tracing::debug;

use crate::sandbox::filesystem::IMAGES_DIR;

/// Information about an available sandbox image.
#[derive(Debug, Clone, Serialize)]
pub struct ImageInfo {
    /// Image name (e.g., "base", "python", "node").
    pub name: String,

    /// File size in bytes.
    pub size_bytes: u64,

    /// Human-readable file size (e.g., "45 MB").
    pub size_human: String,

    /// Absolute path to the squashfs file.
    pub path: PathBuf,
}

/// Manages the image store — listing, locating, and validating images.
pub struct ImageStore {
    /// Root directory where squashfs images are stored.
    images_dir: PathBuf,
}

impl ImageStore {
    /// Creates a new ImageStore pointing at the default images directory.
    pub fn new() -> Self {
        Self {
            images_dir: PathBuf::from(IMAGES_DIR),
        }
    }

    /// Creates an ImageStore with a custom directory (useful for testing).
    pub fn with_dir(path: impl Into<PathBuf>) -> Self {
        Self {
            images_dir: path.into(),
        }
    }

    /// Returns the path to the squashfs file for the given image name.
    ///
    /// Returns an error if the image does not exist.
    pub fn image_path(&self, name: &str) -> Result<PathBuf> {
        let path = self.images_dir.join(format!("{name}.squashfs"));
        if !path.exists() {
            bail!(
                "image '{}' not found at {} — run 'hivebox image build {}' first",
                name,
                path.display(),
                name
            );
        }
        Ok(path)
    }

    /// Checks whether an image exists in the store.
    pub fn exists(&self, name: &str) -> bool {
        self.images_dir.join(format!("{name}.squashfs")).exists()
    }

    /// Lists all available images in the store.
    pub fn list(&self) -> Result<Vec<ImageInfo>> {
        let mut images = Vec::new();

        if !self.images_dir.exists() {
            return Ok(images);
        }

        let entries = fs::read_dir(&self.images_dir)
            .with_context(|| format!("failed to read images dir: {}", self.images_dir.display()))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // Only include .squashfs files.
            if path.extension().is_some_and(|ext| ext == "squashfs") {
                if let Some(stem) = path.file_stem() {
                    let metadata = fs::metadata(&path)?;
                    let size = metadata.len();

                    images.push(ImageInfo {
                        name: stem.to_string_lossy().to_string(),
                        size_bytes: size,
                        size_human: format_size(size),
                        path,
                    });
                }
            }
        }

        // Sort alphabetically by name.
        images.sort_by(|a, b| a.name.cmp(&b.name));

        debug!(count = images.len(), "listed images");
        Ok(images)
    }

    /// Removes an image from the store.
    pub fn remove(&self, name: &str) -> Result<()> {
        let path = self.image_path(name)?;
        fs::remove_file(&path)
            .with_context(|| format!("failed to remove image: {}", path.display()))?;
        Ok(())
    }

    /// Returns the root directory of the image store.
    pub fn dir(&self) -> &Path {
        &self.images_dir
    }
}

/// Formats a byte size into a human-readable string.
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.0} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5 MB");
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.0 GB");
    }
}
