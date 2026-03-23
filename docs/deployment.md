# Deployment Guide

## Requirements

- **OS**: Any Linux with kernel 5.15+ (Debian, Ubuntu, etc.)
- **Kernel features**: namespaces, cgroup v2, overlayfs, squashfs
- **Runtime**: iproute2, iptables, util-linux, squashfs-tools

## Installation Methods

### From Source

```bash
# Install Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add x86_64-unknown-linux-musl

# Build
git clone https://github.com/hivebox/hivebox.git
cd hivebox
cargo build --release --target x86_64-unknown-linux-musl

# Install
sudo cp target/x86_64-unknown-linux-musl/release/hivebox /usr/bin/
```

### Docker (pre-built image)

```bash
docker run --privileged --cgroupns=host -p 7070:7070 \
  -e HIVEBOX_API_KEY=your-secret-key \
  -e HIVEBOX_PACKAGES="python3 nodejs git" \
  ghcr.io/tetiai/hivebox:latest
```

### Docker (build from source)

```bash
docker build -t hivebox .
docker run --privileged --cgroupns=host -p 7070:7070 \
  -e HIVEBOX_API_KEY=your-secret-key \
  hivebox
```

### One-line setup

```bash
curl -fsSL https://raw.githubusercontent.com/TetiAI/hivebox/main/scripts/setup.sh | \
  HIVEBOX_API_KEY=your-key \
  HIVEBOX_PACKAGES="python3 nodejs git" \
  sudo -E bash
```

### Docker Compose

```bash
HIVEBOX_API_KEY=your-secret-key docker compose up -d
```

## Initial Setup

### 1. Create directories

```bash
sudo mkdir -p /var/lib/hivebox/{images,sandboxes,network}
sudo mkdir -p /etc/hivebox
```

### 2. Build rootfs image

```bash
sudo bash scripts/build-images.sh
```

This creates the base squashfs image in `/var/lib/hivebox/images/`:
- `base.squashfs` — Debian minimal (~30 MB)

To pre-install packages in all sandboxes, add them to the Dockerfile or the base image build script (`images/base.sh`). For per-sandbox packages, use `hivebox exec <sandbox> -- apt-get install <package>`.

### 3. Configure

```bash
sudo cp config/hivebox.toml /etc/hivebox/hivebox.toml
# Edit as needed
sudo vim /etc/hivebox/hivebox.toml
```

### 4. Set up the API key

```bash
echo 'HIVEBOX_API_KEY=your-secret-key-here' | sudo tee /etc/hivebox/env
sudo chmod 600 /etc/hivebox/env
```

### 5. Start the daemon

**Manual**:
```bash
hivebox daemon --port 7070
```

**OpenRC**:
```bash
sudo cp config/hivebox.openrc /etc/init.d/hivebox
sudo chmod +x /etc/init.d/hivebox
sudo rc-update add hivebox default
sudo rc-service hivebox start
```

**systemd**:
```ini
# /etc/systemd/system/hivebox.service
[Unit]
Description=HiveBox Sandbox Daemon
After=network.target

[Service]
Type=simple
EnvironmentFile=/etc/hivebox/env
ExecStart=/usr/bin/hivebox daemon --port 7070
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable --now hivebox
```

## Production Checklist

- [ ] Set a strong API key (`--api-key` or `HIVEBOX_API_KEY`)
- [ ] Build the base squashfs image (`scripts/build-images.sh`)
- [ ] Configure firewall to only expose port 7070 to trusted networks
- [ ] Set up log rotation for `/var/log/hivebox.log`
- [ ] Monitor disk space at `/var/lib/hivebox/`
- [ ] Consider running behind a reverse proxy (nginx/caddy) with TLS
- [ ] Set appropriate sandbox timeout limits in config
- [ ] Verify cgroup v2 is mounted: `mount | grep cgroup2`

## Networking Setup

For isolated/shared network modes, ensure:

```bash
# Enable IP forwarding
echo 1 > /proc/sys/net/ipv4/ip_forward

# Load iptables modules
modprobe iptable_nat
modprobe iptable_filter
```

HiveBox automatically configures bridges and NAT rules, but the kernel modules must be available.

## Troubleshooting

### "clone() failed — are user namespaces enabled?"

```bash
# Check if user namespaces are enabled
sysctl kernel.unprivileged_userns_clone
# Should be 1. If not:
sudo sysctl -w kernel.unprivileged_userns_clone=1
```

### "failed to mount overlayfs"

Ensure the squashfs image exists:
```bash
ls /var/lib/hivebox/images/base.squashfs
```

If not, build images first: `sudo bash scripts/build-images.sh`

### "failed to create cgroup"

Ensure cgroup v2 is mounted:
```bash
mount | grep cgroup2
# Should show: cgroup2 on /sys/fs/cgroup type cgroup2
```

### Sandbox stuck / won't destroy

Force cleanup:
```bash
# Find the init process
ps aux | grep "sleep infinity"
# Kill it
sudo kill -9 <PID>
# Remove leftover directory
sudo rm -rf /var/lib/hivebox/sandboxes/hb-*
```
