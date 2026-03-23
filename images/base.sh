#!/bin/sh
# Build the base HiveBox sandbox image.
#
# Creates a minimal Debian rootfs using debootstrap and packages it as squashfs.
# This is the foundation for all other images — it contains apt, coreutils,
# and the minimal Debian userspace with glibc.
#
# Requirements: debootstrap, squashfs-tools (mksquashfs)
# Must run as root (or in a build container).
#
# Usage: ./images/base.sh [output_dir]
#
# Output: {output_dir}/base.squashfs (~25-35 MB)

set -eu

DEBIAN_SUITE="${DEBIAN_SUITE:-bookworm}"
DEBIAN_MIRROR="${DEBIAN_MIRROR:-http://deb.debian.org/debian}"
ARCH="${ARCH:-amd64}"
OUTPUT_DIR="${1:-/var/lib/hivebox/images}"
WORK_DIR="$(mktemp -d)"

cleanup() {
    echo "Cleaning up build directory..."
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

ROOTFS_DIR="$WORK_DIR/rootfs"

echo "=== Building base HiveBox image ==="
echo "Debian suite: ${DEBIAN_SUITE}"
echo "Architecture: ${ARCH}"

# Create minimal Debian rootfs using debootstrap.
echo "Running debootstrap (this may take a minute)..."
debootstrap \
    --variant=minbase \
    --arch="$ARCH" \
    --include=apt,ca-certificates,curl,wget,procps \
    "$DEBIAN_SUITE" \
    "$ROOTFS_DIR" \
    "$DEBIAN_MIRROR"

# Set up DNS resolution.
echo "nameserver 8.8.8.8" > "$ROOTFS_DIR/etc/resolv.conf"
echo "nameserver 1.1.1.1" >> "$ROOTFS_DIR/etc/resolv.conf"

# Create standard directories that sandbox processes expect.
mkdir -p "$ROOTFS_DIR/tmp"
mkdir -p "$ROOTFS_DIR/var/tmp"
mkdir -p "$ROOTFS_DIR/run"

# Set a default hostname (will be overridden by the sandbox).
echo "hivebox" > "$ROOTFS_DIR/etc/hostname"

# Slim down the rootfs: remove docs, man pages, locale data, apt cache.
rm -rf "$ROOTFS_DIR/usr/share/doc"
rm -rf "$ROOTFS_DIR/usr/share/man"
rm -rf "$ROOTFS_DIR/usr/share/info"
rm -rf "$ROOTFS_DIR/usr/share/locale"
rm -rf "$ROOTFS_DIR/var/cache/apt"
rm -rf "$ROOTFS_DIR/var/lib/apt/lists"
mkdir -p "$ROOTFS_DIR/var/cache/apt/archives/partial"
mkdir -p "$ROOTFS_DIR/var/lib/apt/lists/partial"

# Package as squashfs with zstd compression for fast decompression.
echo "Creating squashfs image..."
mkdir -p "$OUTPUT_DIR"
mksquashfs "$ROOTFS_DIR" "$OUTPUT_DIR/base.squashfs" \
    -comp zstd \
    -Xcompression-level 19 \
    -noappend \
    -quiet

SIZE=$(du -sh "$OUTPUT_DIR/base.squashfs" | cut -f1)
echo "=== Base image built: $OUTPUT_DIR/base.squashfs ($SIZE) ==="
