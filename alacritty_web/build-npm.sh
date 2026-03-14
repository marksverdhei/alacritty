#!/usr/bin/env bash
# Build the @alacritty/web npm package using wasm-pack.
#
# Usage:
#   bash build-npm.sh          # default build
#   bash build-npm.sh --release # release (optimized) build

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Check for wasm-pack
if ! command -v wasm-pack &>/dev/null; then
  echo "Error: wasm-pack is not installed."
  echo "Install it with: cargo install wasm-pack"
  exit 1
fi

echo "==> Building alacritty_web with wasm-pack..."
wasm-pack build --target web --out-dir pkg "$@"

# Patch the generated package.json with our metadata.
# wasm-pack generates its own package.json in pkg/; we overwrite the key fields
# so that the published package has the correct name, version, and entry points.
echo "==> Patching pkg/package.json..."

# Read version from the root package.json
VERSION=$(python3 -c "import json; print(json.load(open('package.json'))['version'])" 2>/dev/null || echo "0.1.0")

# Use node if available, otherwise python3, to patch the generated package.json
if command -v node &>/dev/null; then
  node -e "
    const fs = require('fs');
    const pkg = JSON.parse(fs.readFileSync('pkg/package.json', 'utf8'));
    pkg.name = '@alacritty/web';
    pkg.version = '${VERSION}';
    pkg.description = 'Alacritty terminal emulator for the browser via WebAssembly';
    pkg.license = 'Apache-2.0';
    pkg.repository = {
      type: 'git',
      url: 'https://github.com/alacritty/alacritty.git',
      directory: 'alacritty_web'
    };
    pkg.keywords = ['terminal', 'alacritty', 'wasm', 'webassembly', 'canvas'];
    fs.writeFileSync('pkg/package.json', JSON.stringify(pkg, null, 2) + '\n');
  "
elif command -v python3 &>/dev/null; then
  python3 -c "
import json, pathlib
p = pathlib.Path('pkg/package.json')
pkg = json.loads(p.read_text())
pkg['name'] = '@alacritty/web'
pkg['version'] = '${VERSION}'
pkg['description'] = 'Alacritty terminal emulator for the browser via WebAssembly'
pkg['license'] = 'Apache-2.0'
pkg['repository'] = {
    'type': 'git',
    'url': 'https://github.com/alacritty/alacritty.git',
    'directory': 'alacritty_web'
}
pkg['keywords'] = ['terminal', 'alacritty', 'wasm', 'webassembly', 'canvas']
p.write_text(json.dumps(pkg, indent=2) + '\n')
  "
else
  echo "Warning: Neither node nor python3 available; pkg/package.json not patched."
fi

echo "==> Build complete. Output in pkg/"
echo ""
echo "Package contents:"
ls -lh pkg/*.js pkg/*.d.ts pkg/*.wasm 2>/dev/null || true
