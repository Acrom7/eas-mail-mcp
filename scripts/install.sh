#!/bin/sh
set -eu

VERSION="0.1.2"
CLIENTS=""
REMOVE_QUARANTINE=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --clients)
      [ "$#" -ge 2 ] || { printf '%s\n' "--clients requires a comma-separated value" >&2; exit 2; }
      CLIENTS="$2"
      shift 2
      ;;
    --remove-quarantine)
      REMOVE_QUARANTINE=1
      shift
      ;;
    *)
      printf '%s\n' "unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
cd "$ROOT"
shasum -a 256 -c SHA256SUMS

ARCH=$(uname -m)
if [ -f TARGET_ARCH ]; then
  EXPECTED_ARCH=$(cat TARGET_ARCH)
  [ "$ARCH" = "$EXPECTED_ARCH" ] || {
    printf '%s\n' "This bundle targets $EXPECTED_ARCH, current Mac is $ARCH" >&2
    exit 1
  }
  SOURCE_BINARY="bin/eas-mail-mcp"
elif [ -f TARGET_ARCHS ]; then
  case "$ARCH" in
    arm64|x86_64) SOURCE_BINARY="bin/$ARCH/eas-mail-mcp" ;;
    *) printf '%s\n' "Unsupported Mac architecture: $ARCH" >&2; exit 1 ;;
  esac
  grep -qx "$ARCH" TARGET_ARCHS || {
    printf '%s\n' "This handoff does not contain a binary for $ARCH" >&2
    exit 1
  }
else
  printf '%s\n' "Bundle architecture metadata is missing" >&2
  exit 1
fi

[ -f "$SOURCE_BINARY" ] && [ ! -L "$SOURCE_BINARY" ] || {
  printf '%s\n' "Selected bundle binary is missing or is not a regular file" >&2
  exit 1
}

if xattr -p com.apple.quarantine "$ROOT" >/dev/null 2>&1; then
  if [ "$REMOVE_QUARANTINE" -eq 0 ]; then
    if [ -t 0 ]; then
      printf '%s' "Hash verified. Remove quarantine from this unsigned pilot bundle? [y/N] " >&2
      read -r answer
      case "$answer" in y|Y|yes|YES) ;; *) exit 1 ;; esac
    else
      printf '%s\n' "Bundle is quarantined; rerun with --remove-quarantine after verifying its hash" >&2
      exit 1
    fi
  fi
  xattr -dr com.apple.quarantine "$ROOT"
fi

BASE=${EAS_MAIL_MCP_HOME:-"$HOME/.local/lib/eas-mail-mcp"}
DESTINATION="$BASE/$VERSION"
BIN_DIR=${EAS_MAIL_MCP_BIN_DIR:-"$HOME/.local/bin"}
SUPPORT="$HOME/Library/Application Support/EAS Mail MCP"
STAMP=$(date -u +%Y%m%dT%H%M%SZ)-$$
TARGET_BINARY="$DESTINATION/bin/eas-mail-mcp"
REUSE_BINARY=0

if [ -e "$TARGET_BINARY" ] || [ -L "$TARGET_BINARY" ]; then
  if [ -L "$TARGET_BINARY" ] || [ ! -f "$TARGET_BINARY" ] || \
     ! cmp -s "$SOURCE_BINARY" "$TARGET_BINARY"; then
    printf '%s\n' "Version $VERSION already exists with different contents" >&2
    exit 1
  fi
  REUSE_BINARY=1
fi

mkdir -p "$DESTINATION/bin" "$DESTINATION/share" "$BIN_DIR" "$SUPPORT/Install Backups/$STAMP"
chmod 700 "$SUPPORT" "$SUPPORT/Install Backups" "$SUPPORT/Install Backups/$STAMP"
if [ -f "$SUPPORT/config.toml" ]; then
  cp -p "$SUPPORT/config.toml" "$SUPPORT/Install Backups/$STAMP/config.toml"
fi
if [ -L "$BIN_DIR/eas-mail-mcp" ] || [ -f "$BIN_DIR/eas-mail-mcp" ]; then
  cp -p "$BIN_DIR/eas-mail-mcp" "$SUPPORT/Install Backups/$STAMP/previous-binary"
fi

TEMP_BINARY="$DESTINATION/bin/.eas-mail-mcp.$$"
TEMP_LINK="$BIN_DIR/.eas-mail-mcp.$$"
trap 'rm -f "$TEMP_BINARY" "$TEMP_LINK"' 0
trap 'exit 1' 1 2 15
if [ "$REUSE_BINARY" -eq 0 ]; then
  cp "$SOURCE_BINARY" "$TEMP_BINARY"
  chmod 700 "$TEMP_BINARY"
  mv -f "$TEMP_BINARY" "$TARGET_BINARY"
fi
cp uninstall.sh "$DESTINATION/share/uninstall.sh"
cp installation.ru.md "$DESTINATION/share/installation.ru.md"
chmod 700 "$TARGET_BINARY" "$DESTINATION/share/uninstall.sh"
ln -s "$TARGET_BINARY" "$TEMP_LINK"
mv -f "$TEMP_LINK" "$BIN_DIR/eas-mail-mcp"

OLD_IFS=$IFS
IFS=,
for client in $CLIENTS; do
  [ -n "$client" ] || continue
  "$DESTINATION/bin/eas-mail-mcp" client configure "$client"
done
IFS=$OLD_IFS

printf '%s\n' "Installed $DESTINATION/bin/eas-mail-mcp"
printf '%s\n' "Run: $BIN_DIR/eas-mail-mcp setup"
