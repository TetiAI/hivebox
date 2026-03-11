#!/bin/sh
# HiveBox container entrypoint.
# Installs extra packages into the sandbox base squashfs image so every
# sandbox gets them. Packages are only installed once — a stamp file
# tracks the current configuration to skip rebuilds on restart.

set -e

SQUASHFS="/var/lib/hivebox/images/base.squashfs"
STAMP="/var/lib/hivebox/images/.packages-installed"

# Check if we need to rebuild the squashfs.
current_sig="${HIVEBOX_PACKAGES:-}|${HIVEBOX_PIP_PACKAGES:-}|${HIVEBOX_NPM_PACKAGES:-}"
if [ -n "$current_sig" ] && [ "$current_sig" != "||" ]; then
    if [ ! -f "$STAMP" ] || [ "$(cat "$STAMP")" != "$current_sig" ]; then
        echo "[hivebox] Rebuilding sandbox base image with custom packages..."
        WORK_DIR="$(mktemp -d)"
        ROOTFS="$WORK_DIR/rootfs"

        # Extract existing squashfs.
        unsquashfs -d "$ROOTFS" "$SQUASHFS" > /dev/null 2>&1 || {
            unsquashfs -d "$WORK_DIR/sq" "$SQUASHFS" > /dev/null 2>&1
            rm -rf "$ROOTFS"
            mv "$WORK_DIR/sq" "$ROOTFS"
        }

        # Set up DNS for package installation.
        cp /etc/resolv.conf "$ROOTFS/etc/resolv.conf" 2>/dev/null || true

        # 1. Alpine packages (apk is already in the minirootfs).
        if [ -n "${HIVEBOX_PACKAGES:-}" ]; then
            echo "[hivebox]   -> Alpine packages: $HIVEBOX_PACKAGES"
            chroot "$ROOTFS" /bin/sh -c "apk add --no-cache $HIVEBOX_PACKAGES" || echo "[hivebox] WARNING: some Alpine packages failed"
        fi

        # 2. pip packages (python3/pip must be in HIVEBOX_PACKAGES).
        if [ -n "${HIVEBOX_PIP_PACKAGES:-}" ]; then
            echo "[hivebox]   -> pip packages: $HIVEBOX_PIP_PACKAGES"
            chroot "$ROOTFS" /bin/sh -c "pip install --no-cache-dir --break-system-packages $HIVEBOX_PIP_PACKAGES" || echo "[hivebox] WARNING: some pip packages failed"
        fi

        # 3. npm packages (nodejs/npm must be in HIVEBOX_PACKAGES).
        if [ -n "${HIVEBOX_NPM_PACKAGES:-}" ]; then
            echo "[hivebox]   -> npm packages: $HIVEBOX_NPM_PACKAGES"
            chroot "$ROOTFS" /bin/sh -c "npm install -g $HIVEBOX_NPM_PACKAGES" || echo "[hivebox] WARNING: some npm packages failed"
        fi

        # Repackage as squashfs (fast compression for startup speed).
        echo "[hivebox]   -> Repackaging squashfs..."
        mksquashfs "$ROOTFS" "$SQUASHFS.new" -comp zstd -Xcompression-level 3 -noappend -quiet
        mv "$SQUASHFS.new" "$SQUASHFS"
        rm -rf "$WORK_DIR"

        echo "$current_sig" > "$STAMP"
        echo "[hivebox] Sandbox base image rebuilt."
    else
        echo "[hivebox] Sandbox base image up to date (skipping rebuild)."
    fi
fi

# Hand off to hivebox binary.
exec hivebox "$@"
