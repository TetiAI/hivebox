#!/bin/bash
# HiveBox — one-line setup for any Linux VPS.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/TetiAI/hivebox/main/scripts/setup.sh | bash
#
# With custom packages and OpenCode:
#   curl -fsSL ... | HIVEBOX_PACKAGES="python3 nodejs git" HIVEBOX_OPENCODE_API_KEY=sk-... bash
#
# What it does:
#   1. Installs Docker (if not present)
#   2. Pulls the latest HiveBox image from GHCR
#   3. Generates a random API key
#   4. Starts HiveBox with docker compose
#
# Requirements: Linux with kernel 5.15+, root or sudo access.

set -euo pipefail

HIVEBOX_IMAGE="ghcr.io/tetiai/hivebox:latest"
INSTALL_DIR="/opt/hivebox"

# --- Colors ---
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${CYAN}[hivebox]${NC} $1"; }
ok()    { echo -e "${GREEN}[hivebox]${NC} $1"; }
warn()  { echo -e "${YELLOW}[hivebox]${NC} $1"; }
fail()  { echo -e "${RED}[hivebox]${NC} $1"; exit 1; }

# --- Root check ---
if [ "$(id -u)" -ne 0 ]; then
    fail "Please run as root: sudo bash or curl ... | sudo bash"
fi

# --- Kernel check ---
KVER=$(uname -r | cut -d. -f1-2)
KMAJOR=$(echo "$KVER" | cut -d. -f1)
KMINOR=$(echo "$KVER" | cut -d. -f2)
if [ "$KMAJOR" -lt 5 ] || { [ "$KMAJOR" -eq 5 ] && [ "$KMINOR" -lt 15 ]; }; then
    fail "Kernel $KVER detected. HiveBox requires Linux 5.15+."
fi
ok "Kernel $KVER — OK"

# --- Install Docker if missing ---
if ! command -v docker &>/dev/null; then
    info "Docker not found. Installing..."
    curl -fsSL https://get.docker.com | sh
    systemctl enable --now docker
    ok "Docker installed"
else
    ok "Docker found: $(docker --version | head -1)"
fi

# --- Check docker compose ---
if docker compose version &>/dev/null; then
    COMPOSE="docker compose"
elif command -v docker-compose &>/dev/null; then
    COMPOSE="docker-compose"
else
    fail "docker compose not found. Please install Docker Compose v2."
fi
ok "Compose: $($COMPOSE version | head -1)"

# --- Create install directory ---
mkdir -p "$INSTALL_DIR"
cd "$INSTALL_DIR"

# --- Generate .env file ---
# Respects environment variables passed by the user; generates defaults otherwise.
if [ -f .env ] && [ -z "${HIVEBOX_FORCE:-}" ]; then
    info "Existing .env found, keeping current config (set HIVEBOX_FORCE=1 to overwrite)"
    # shellcheck disable=SC1091
    source .env
else
    HIVEBOX_API_KEY="${HIVEBOX_API_KEY:-$(openssl rand -hex 24)}"
    cat > .env <<EOF
HIVEBOX_API_KEY=${HIVEBOX_API_KEY}
HIVEBOX_OPENCODE=${HIVEBOX_OPENCODE:-true}
HIVEBOX_OPENCODE_API_KEY=${HIVEBOX_OPENCODE_API_KEY:-}
HIVEBOX_OPENCODE_BASE_URL=${HIVEBOX_OPENCODE_BASE_URL:-}
HIVEBOX_OPENCODE_MODEL=${HIVEBOX_OPENCODE_MODEL:-}
HIVEBOX_PACKAGES=${HIVEBOX_PACKAGES:-}
HIVEBOX_PIP_PACKAGES=${HIVEBOX_PIP_PACKAGES:-}
HIVEBOX_NPM_PACKAGES=${HIVEBOX_NPM_PACKAGES:-}
HIVEBOX_OPENCODE_SKILLS_PATH=${HIVEBOX_OPENCODE_SKILLS_PATH:-}
RUST_LOG=${RUST_LOG:-info}
EOF
    ok "Wrote .env"
fi

# --- Write docker-compose.yml ---
# Build volumes list — always include base volumes, add skills mount if configured.
SKILLS_VOLUME=""
if [ -n "${HIVEBOX_OPENCODE_SKILLS_PATH:-}" ]; then
    SKILLS_VOLUME="      - ${HIVEBOX_OPENCODE_SKILLS_PATH}:/opt/hivebox/skills:ro"
    info "Custom skills directory: ${HIVEBOX_OPENCODE_SKILLS_PATH} -> /opt/hivebox/skills"
fi

cat > docker-compose.yml <<YAML
services:
  hivebox:
    image: ghcr.io/tetiai/hivebox:latest
    container_name: hivebox
    privileged: true
    cgroup_parent: ""
    cgroup: host
    ports:
      - "7070:7070"
    env_file: .env
    volumes:
      - hivebox-images:/var/lib/hivebox/images
      - /sys/fs/cgroup:/sys/fs/cgroup:rw
${SKILLS_VOLUME:+$SKILLS_VOLUME}
    healthcheck:
      test: ["CMD", "wget", "-qO-", "http://localhost:7070/healthz"]
      interval: 30s
      timeout: 5s
      retries: 3
    restart: unless-stopped

volumes:
  hivebox-images:
YAML
ok "Wrote $INSTALL_DIR/docker-compose.yml"

# --- Stop existing and clean up if force ---
if [ -n "${HIVEBOX_FORCE:-}" ]; then
    info "Force mode: stopping existing container and clearing cached images..."
    $COMPOSE down -v 2>/dev/null || true
fi

# --- Pull and start ---
info "Pulling $HIVEBOX_IMAGE ..."
$COMPOSE pull

info "Starting HiveBox..."
$COMPOSE up -d

# --- Wait for health ---
info "Waiting for health check..."
for i in $(seq 1 30); do
    if wget -qO- http://localhost:7070/healthz &>/dev/null; then
        break
    fi
    sleep 2
done

if wget -qO- http://localhost:7070/healthz &>/dev/null; then
    echo ""
    ok "========================================="
    ok " HiveBox is running!"
    ok "========================================="
    echo ""
    echo -e " Dashboard:  ${CYAN}http://$(hostname -I | awk '{print $1}'):7070/dashboard${NC}"
    echo -e " API:        ${CYAN}http://$(hostname -I | awk '{print $1}'):7070/api/v1/hiveboxes${NC}"
    echo -e " API Key:    ${YELLOW}${HIVEBOX_API_KEY}${NC}"
    echo ""
    echo -e " Config dir: ${INSTALL_DIR}"
    echo -e " Logs:       ${CYAN}cd $INSTALL_DIR && $COMPOSE logs -f${NC}"
    echo -e " Stop:       ${CYAN}cd $INSTALL_DIR && $COMPOSE down${NC}"
    echo -e " Update:     ${CYAN}cd $INSTALL_DIR && $COMPOSE pull && $COMPOSE up -d${NC}"
    echo ""
else
    warn "HiveBox started but health check is not responding yet."
    warn "Check logs: cd $INSTALL_DIR && $COMPOSE logs -f"
fi
