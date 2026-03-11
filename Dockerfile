# HiveBox — multi-stage Docker build.
#
# The resulting image runs HiveBox inside a privileged Alpine container.
# This is useful for deploying HiveBox on hosts where you don't want to
# install it directly (e.g., cloud VMs, CI environments).
#
# Build:  docker build -t hivebox .
# Run:    docker run --privileged --cgroupns=host -p 7070:7070 hivebox
#
# The --privileged flag is required because HiveBox uses Linux namespaces,
# cgroups, and mount operations that need elevated permissions.
# The --cgroupns=host flag gives access to the host cgroup hierarchy,
# needed for setting memory/cpu/pid limits on sandboxes.

# --- Stage 1: Build the static binary ---
FROM rust:latest AS builder

RUN apt-get update && apt-get install -y musl-tools && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

# Build a fully static binary using musl.
RUN cargo build --release --target x86_64-unknown-linux-musl

# --- Stage 2: Runtime image ---
FROM alpine:3.21

LABEL org.opencontainers.image.source="https://github.com/TetiAI/hivebox" \
      org.opencontainers.image.description="Native Linux sandboxing built for the AI era" \
      org.opencontainers.image.licenses="MIT"

# Install runtime dependencies for sandbox management.
RUN apk add --no-cache \
    iproute2 \
    iptables \
    util-linux \
    squashfs-tools \
    curl \
    bash \
    libstdc++ \
    libgcc \
    && mkdir -p /var/lib/hivebox/images \
    && mkdir -p /var/lib/hivebox/sandboxes \
    && mkdir -p /var/lib/hivebox/network \
    && mkdir -p /workspace

# Install opencode (AI coding agent — used by opencode serve per hivebox).
RUN curl -fsSL https://opencode.ai/install | bash \
    && ln -sf /root/.opencode/bin/opencode /usr/local/bin/opencode

# Install git (needed to clone skills repo) and download Anthropic skills.
RUN apk add --no-cache git \
    && git clone --depth 1 https://github.com/anthropics/skills.git /tmp/skills \
    && mkdir -p /root/.config/opencode/skills \
    && cp -r /tmp/skills/skills/* /root/.config/opencode/skills/ \
    && rm -rf /tmp/skills

# Copy the static binary from the builder stage.
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/hivebox /usr/bin/hivebox

# Copy image build scripts and build the base squashfs rootfs.
COPY images/ /opt/hivebox/images/
COPY scripts/ /opt/hivebox/scripts/
COPY config/ /etc/hivebox/
RUN sh /opt/hivebox/scripts/build-images.sh

# Make entrypoint executable.
RUN chmod +x /opt/hivebox/scripts/entrypoint.sh

# Expose the API port.
EXPOSE 7070

# Health check.
HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD wget -qO- http://localhost:7070/healthz || exit 1

# Default: start the daemon. Override with docker run args for CLI usage.
ENTRYPOINT ["/opt/hivebox/scripts/entrypoint.sh"]
CMD ["daemon", "--port", "7070"]
