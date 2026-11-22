#!/usr/bin/env bash
# Runs the api and delivery services locally for the app (apps/ui) to talk to:
# registration with proof of work, usernames, devices, invites and the delivery
# WebSocket, against the docker compose stores. Development only.
#
#   cargo xtask compose up            # the stores (once)
#   scripts/dev-app-backend.sh        # api on http://127.0.0.1:8443, delivery on 127.0.0.1:9201
#   scripts/dev-app-backend.sh invite # mint an invite code (prints it once)
#
# Secrets are generated into target/dev-app-backend (git-ignored), never the repository.
set -euo pipefail
cd "$(dirname "$0")/.."

dir=target/dev-app-backend
mkdir -p "$dir"
chmod 700 "$dir"
[ -f .env ] || { echo "run 'cargo xtask compose up' first (it writes .env)" >&2; exit 1; }
set -a; . ./.env; set +a

secret() { # a 64-hex secret file, made once
  local f="$dir/$1.hex"
  [ -f "$f" ] || { (umask 077; openssl rand -hex 32 > "$f"); }
  echo "$PWD/$f"
}

if [ "${1:-}" = "invite" ]; then
  cargo run -q -p songbird-admin -- invites create --addr 127.0.0.1:47800 --count 1 \
    --confirm songbird/songbird --audit-log "$PWD/$dir/admin-audit.jsonl"
  exit 0
fi

push_pub=$(openssl rand -hex 32)
cat > "$dir/api.toml" <<TOML
[common]
environment = "dev"
admin_listen = "127.0.0.1:9102"
shutdown_grace_secs = 5
[common.log]
format = "pretty"
filter = "info"

[api]
listen = "127.0.0.1:8443"
server_name = "api.localhost"
push_public_key = "$push_pub"
push_index_secret = { file = "$(secret push-index)" }
[api.surrealdb]
addr = "127.0.0.1:47800"
host = "localhost"
user = "root"
pass = { env = "SURREAL_PASS" }
timeout_ms = 30000  # Podman on macOS forwards ports slowly at first
[api.valkey_hot]
addr = "127.0.0.1:47379"
pass = { env = "VALKEY_HOT_PASSWORD" }
[api.jobs]
secret = { file = "$(secret jobs)" }
[api.jobs.valkey]
addr = "127.0.0.1:47380"
pass = { env = "VALKEY_JOBS_PASSWORD" }
[api.s3]
addr = "127.0.0.1:47900"
bucket = "songbird-attachments"
access_key = { env = "RUSTFS_ACCESS_KEY" }
secret_key = { env = "RUSTFS_SECRET_KEY" }
[api.registration]
secret = { file = "$(secret registration)" }
invite_only = ${SONGBIRD_INVITE_ONLY:-true}
allowance = 5
[api.usernames]
secret = { file = "$(secret usernames)" }
[api.keys]
secret = { file = "$(secret keys)" }
# Profiles (names, access keys for first messages), synced settings and message requests
# (D-0183): without them the app's first messages are refused.
[api.profiles]
secret = { file = "$(secret profiles)" }
[api.storage]
secret = { file = "$(secret storage)" }
# Attachment downloads (signed media URLs): without it every download answers 503.
[api.media]
secret = { file = "$(secret media)" }
[api.requests]
secret = { file = "$(secret requests)" }
TOML

cat > "$dir/delivery.toml" <<TOML
[common]
environment = "dev"
admin_listen = "127.0.0.1:9101"
shutdown_grace_secs = 5
[common.log]
format = "pretty"
filter = "info"

[delivery]
listen = "127.0.0.1:9201"
server_name = "localhost"
franking_key = { file = "$(secret franking)" }
[delivery.requests]
secret = { file = "$(secret requests)" }
[delivery.requests.valkey]
addr = "127.0.0.1:47379"
pass = { env = "VALKEY_HOT_PASSWORD" }
[delivery.registry]
poll_ms = 1000
[delivery.registry.surrealdb]
addr = "127.0.0.1:47800"
host = "localhost"
user = "root"
pass = { env = "SURREAL_PASS" }
timeout_ms = 30000  # Podman on macOS forwards ports slowly at first
TOML

cargo build -q -p songbird-api -p songbird-delivery
trap 'kill 0' EXIT
target/debug/songbird-api --config "$dir/api.toml" &
target/debug/songbird-delivery --config "$dir/delivery.toml" &
echo "api: http://127.0.0.1:8443 (server name api.localhost) · delivery: 127.0.0.1:9201 (server name localhost)"
wait
