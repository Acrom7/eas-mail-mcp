#!/bin/sh
set -eu

VERSION="0.1.0"
DELETE_DATA=0
UNCONFIGURE_CLIENTS=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --delete-data) DELETE_DATA=1; shift ;;
    --clients)
      [ "$#" -ge 2 ] || { printf '%s\n' "--clients requires a comma-separated value" >&2; exit 2; }
      UNCONFIGURE_CLIENTS="$2"
      shift 2
      ;;
    *) printf '%s\n' "unknown argument: $1" >&2; exit 2 ;;
  esac
done

BASE=${EAS_MAIL_MCP_HOME:-"$HOME/.local/lib/eas-mail-mcp"}
BIN_DIR=${EAS_MAIL_MCP_BIN_DIR:-"$HOME/.local/bin"}
BINARY="$BASE/$VERSION/bin/eas-mail-mcp"

OLD_IFS=$IFS
IFS=,
for client in $UNCONFIGURE_CLIENTS; do
  [ -n "$client" ] || continue
  "$BINARY" client unconfigure "$client"
done
IFS=$OLD_IFS

if [ -L "$BIN_DIR/eas-mail-mcp" ] && \
   [ "$(readlink "$BIN_DIR/eas-mail-mcp")" = "$BINARY" ]; then
  rm "$BIN_DIR/eas-mail-mcp"
fi
rm -rf "$BASE/$VERSION"

if [ "$DELETE_DATA" -eq 1 ]; then
  security delete-generic-password -s eas-mail-mcp -a secrets-v1 >/dev/null 2>&1 || true
  rm -rf "$HOME/Library/Application Support/EAS Mail MCP"
  rm -rf "$HOME/Library/Caches/EAS Mail MCP"
fi

printf '%s\n' "Uninstalled EAS Mail MCP $VERSION"
