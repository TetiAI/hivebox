#!/bin/sh
# Build the base HiveBox sandbox image.
#
# Downloads Alpine Linux minirootfs and packages it as a squashfs image.
# This is the foundation for all other images — it contains busybox, apk,
# and the minimal Alpine userspace.
#
# Requirements: wget, squashfs-tools (mksquashfs), tar
# Must run as root (or in a build container).
#
# Usage: ./images/base.sh [output_dir]
#
# Output: {output_dir}/base.squashfs (~5 MB)

set -eu

ALPINE_VERSION="${ALPINE_VERSION:-3.21}"
ALPINE_MINOR="${ALPINE_MINOR:-3}"
ARCH="${ARCH:-x86_64}"
OUTPUT_DIR="${1:-/var/lib/hivebox/images}"
WORK_DIR="$(mktemp -d)"

cleanup() {
    echo "Cleaning up build directory..."
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

TARBALL_URL="https://dl-cdn.alpinelinux.org/alpine/v${ALPINE_VERSION}/releases/${ARCH}/alpine-minirootfs-${ALPINE_VERSION}.${ALPINE_MINOR}-${ARCH}.tar.gz"
TARBALL_PATH="$WORK_DIR/minirootfs.tar.gz"
ROOTFS_DIR="$WORK_DIR/rootfs"

echo "=== Building base HiveBox image ==="
echo "Alpine version: ${ALPINE_VERSION}.${ALPINE_MINOR}"
echo "Architecture: ${ARCH}"

# Download the Alpine minirootfs tarball.
echo "Downloading Alpine minirootfs..."
wget -q -O "$TARBALL_PATH" "$TARBALL_URL"

# Extract to the rootfs working directory.
echo "Extracting rootfs..."
mkdir -p "$ROOTFS_DIR"
tar xzf "$TARBALL_PATH" -C "$ROOTFS_DIR"

# Set up DNS resolution for package installation.
echo "nameserver 8.8.8.8" > "$ROOTFS_DIR/etc/resolv.conf"
echo "nameserver 1.1.1.1" >> "$ROOTFS_DIR/etc/resolv.conf"


# Create standard directories that sandbox processes expect.
mkdir -p "$ROOTFS_DIR/tmp"
mkdir -p "$ROOTFS_DIR/var/tmp"
mkdir -p "$ROOTFS_DIR/run"

# Set a default hostname (will be overridden by the sandbox).
echo "hivebox" > "$ROOTFS_DIR/etc/hostname"

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
