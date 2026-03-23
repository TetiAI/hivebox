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

        # 1. Debian packages (apt is in the base rootfs).
        if [ -n "${HIVEBOX_PACKAGES:-}" ]; then
            echo "[hivebox]   -> Debian packages: $HIVEBOX_PACKAGES"
            chroot "$ROOTFS" /bin/sh -c "apt-get update && apt-get install -y --no-install-recommends $HIVEBOX_PACKAGES && rm -rf /var/lib/apt/lists/*" || echo "[hivebox] WARNING: some Debian packages failed"
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

        # Set NODE_OPTIONS to limit V8 memory (prevents OOM in constrained sandboxes).
        echo 'export NODE_OPTIONS="--max-old-space-size=256"' >> "$ROOTFS/etc/profile"
        mkdir -p "$ROOTFS/etc/profile.d"
        echo 'export NODE_OPTIONS="--max-old-space-size=256"' > "$ROOTFS/etc/profile.d/node.sh"

        # Repackage as squashfs (fast compression for startup speed).
        echo "[hivebox]   -> Repackaging squashfs..."
        mksquashfs "$ROOTFS" "$SQUASHFS.new" -comp zstd -Xcompression-level 3 -noappend -quiet
        mv "$SQUASHFS.new" "$SQUASHFS"
        rm -rf "$WORK_DIR"

        # Remove the shared rootfs cache so hivebox re-extracts from the new squashfs.
        rm -rf /var/lib/hivebox/images/base.rootfs

        echo "$current_sig" > "$STAMP"
        echo "[hivebox] Sandbox base image rebuilt."
    else
        echo "[hivebox] Sandbox base image up to date (skipping rebuild)."
    fi
fi

# Pre-extract squashfs into shared cache so the first sandbox starts instantly.
ROOTFS_CACHE="/var/lib/hivebox/images/base.rootfs"
if [ -f "$SQUASHFS" ] && [ ! -d "$ROOTFS_CACHE" ]; then
    echo "[hivebox] Pre-extracting squashfs to shared cache..."
    unsquashfs -d "$ROOTFS_CACHE" "$SQUASHFS" > /dev/null 2>&1 || true
    echo "[hivebox] Shared cache ready."
fi

# Hand off to hivebox binary.
exec hivebox "$@"
