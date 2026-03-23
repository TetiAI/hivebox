//! Image building — creates squashfs images from build scripts.
//!
//! Each image has a corresponding shell script in the `images/` directory
//! (e.g., `images/python.sh`) that uses debootstrap to create a Debian rootfs, installs
//! packages, and produces a `.squashfs` file.
//!
//! This module orchestrates running those scripts.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use tracing::info;

/// Builds a sandbox image by running its build script.
///
/// The build script is expected to be at `{scripts_dir}/{name}.sh` and produce
/// a squashfs file at `{output_dir}/{name}.squashfs`.
///
/// # Requirements
///
/// - Must run as root (build scripts use chroot and mount)
/// - Must be on a Debian-based system or in a Debian container (for debootstrap)
/// - Packages: squashfs-tools, debootstrap
pub fn build_image(name: &str, scripts_dir: &Path, output_dir: &Path) -> Result<()> {
    let script_path = scripts_dir.join(format!("{name}.sh"));

    if !script_path.exists() {
        bail!(
            "no build script for image '{}' (expected {})",
            name,
            script_path.display()
        );
    }

    info!(
        image = name,
        script = %script_path.display(),
        output = %output_dir.display(),
        "building image"
    );

    let output = Command::new("sh")
        .arg(&script_path)
        .arg(output_dir)
        .output()
        .with_context(|| format!("failed to execute build script: {}", script_path.display()))?;

    // Print build output for visibility.
    if !output.stdout.is_empty() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            info!(image = name, "{}", line);
        }
    }

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "image build failed for '{}' (exit code {:?}):\n{}",
            name,
            output.status.code(),
            stderr
        );
    }

    // Verify the output file was created.
    let output_file = output_dir.join(format!("{name}.squashfs"));
    if !output_file.exists() {
        bail!(
            "build script succeeded but output file not found: {}",
            output_file.display()
        );
    }

    info!(
        image = name,
        path = %output_file.display(),
        "image built successfully"
    );
    Ok(())
}
