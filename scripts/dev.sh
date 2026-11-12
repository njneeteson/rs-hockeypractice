#!/usr/bin/env sh
# Run the server locally at http://127.0.0.1:8080/localapp with data in ./data.
# Plain HTTP, so secure cookies are off. The first-run setup link is printed below.
#
#   ./scripts/dev.sh
#   ./scripts/dev.sh admin list       # any localapp-server command
set -eu
cd "$(dirname "$0")/.."
# Git Bash on Windows would otherwise rewrite /localapp into a Windows path.
MSYS_NO_PATHCONV=1 \
MSYS2_ENV_CONV_EXCL=LOCALAPP_BASE_PATH \
LOCALAPP_DATA_DIR=./data \
LOCALAPP_BASE_PATH=/localapp \
LOCALAPP_BIND=127.0.0.1:8080 \
LOCALAPP_SECURE_COOKIES=false \
RUST_LOG="${RUST_LOG:-info,tower_http=debug}" \
exec cargo run -p localapp-server -- "$@"
