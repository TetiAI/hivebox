#!/bin/sh
# Build the HiveBox base rootfs image.
#
# Creates the Debian base squashfs image used as the read-only lower
# layer for sandbox overlayfs mounts.
#
# Requirements:
#   - Must run as root (for chroot and mount operations)
#   - Must be on a Debian-based system or inside a Debian container (for debootstrap)
#   - Packages: squashfs-tools debootstrap
#
# Usage:
#   ./scripts/build-images.sh
#
# The image is written to /var/lib/hivebox/images/ by default.
# Override with the HIVEBOX_IMAGES_DIR environment variable.
#
# To pre-install packages in all sandboxes, edit images/base.sh.
# For per-sandbox packages, use: hivebox exec <sandbox> -- apt-get install <package>

set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
IMAGES_DIR="${HIVEBOX_IMAGES_DIR:-/var/lib/hivebox/images}"
IMAGES_TO_BUILD="base"

echo "=== HiveBox Image Builder ==="
echo "Output directory: $IMAGES_DIR"
echo "Images to build: $IMAGES_TO_BUILD"
echo ""

# Verify we have the required tools.
for tool in debootstrap mksquashfs; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "ERROR: Required tool '$tool' not found."
        echo "Install with: apt-get install squashfs-tools debootstrap"
        exit 1
    fi
done

# Verify we're running as root (needed for chroot and mount).
if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: Must run as root (needed for chroot and mount operations)."
    echo "Try: sudo $0 $*"
    exit 1
fi

mkdir -p "$IMAGES_DIR"

FAILED=""
for image in $IMAGES_TO_BUILD; do
    script="$SCRIPT_DIR/images/${image}.sh"
    if [ ! -f "$script" ]; then
        echo "WARNING: No build script for image '$image' (expected $script)"
        FAILED="$FAILED $image"
        continue
    fi

    echo ""
    echo "--- Building image: $image ---"
    if sh "$script" "$IMAGES_DIR"; then
        echo "--- Image '$image' built successfully ---"
    else
        echo "ERROR: Failed to build image '$image'"
        FAILED="$FAILED $image"
    fi
done

echo ""
echo "=== Build Summary ==="
echo "Output directory: $IMAGES_DIR"
ls -lh "$IMAGES_DIR"/*.squashfs 2>/dev/null || echo "(no images built)"

if [ -n "$FAILED" ]; then
    echo ""
    echo "FAILED images:$FAILED"
    exit 1
fi

echo ""
echo "All images built successfully."
