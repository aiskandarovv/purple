#!/usr/bin/env bash
# Build a .mcpb (MCP Bundle) for Claude Desktop from a release binary.
#
# Usage:
#   scripts/build-mcpb.sh <version> <target-triple> <binary-path>
#
# Example:
#   scripts/build-mcpb.sh 2.45.0 aarch64-apple-darwin target/release/purple
#
# Output: purple-<version>-<target>.mcpb in the current directory.
#
# Requires:
#   npx (Node.js) for the official @anthropic-ai/mcpb pack tool.

set -euo pipefail

VERSION="${1:?missing version (e.g., 2.45.0)}"
TARGET="${2:?missing target triple (e.g., aarch64-apple-darwin)}"
BINARY="${3:?missing binary path}"

if [ ! -x "$BINARY" ]; then
  echo "error: binary not found or not executable: $BINARY" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEMPLATE="$REPO_ROOT/mcpb/manifest.template.json"
ICON="$REPO_ROOT/mcpb/icon.png"

if [ ! -f "$TEMPLATE" ]; then
  echo "error: manifest template not found: $TEMPLATE" >&2
  exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Render the manifest with the version
sed "s/__VERSION__/$VERSION/g" "$TEMPLATE" > "$WORK/manifest.json"

# Bundle layout. macOS and Linux only: Windows is not a supported target.
mkdir -p "$WORK/server"
cp "$BINARY" "$WORK/server/purple"
chmod +x "$WORK/server/purple"

if [ -f "$ICON" ]; then
  cp "$ICON" "$WORK/icon.png"
fi

# Pack via the official tool. --output writes to a stable name we control.
OUTPUT="purple-${VERSION}-${TARGET}.mcpb"
( cd "$WORK" && npx --yes @anthropic-ai/mcpb pack . "$OLDPWD/$OUTPUT" )

echo "built: $OUTPUT"
ls -lh "$OUTPUT"
