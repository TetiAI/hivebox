#!/bin/sh
# HiveBox container entrypoint.
# Installs extra Alpine packages (if requested) before starting the daemon.

set -e

# Install extra packages into the base squashfs overlay if HIVEBOX_PACKAGES is set.
if [ -n "${HIVEBOX_PACKAGES:-}" ]; then
    echo "[hivebox] Installing extra packages: $HIVEBOX_PACKAGES"
    apk add --no-cache $HIVEBOX_PACKAGES
fi

# Hand off to hivebox binary with whatever args were passed.
exec hivebox "$@"
