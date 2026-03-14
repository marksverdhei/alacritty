#!/bin/bash
# Build the WASM package and set up the demo site.
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "Building alacritty_web WASM package..."
cd "$PROJECT_DIR/alacritty_web"
wasm-pack build --target web

echo "Copying pkg to demo directory..."
rm -rf "$SCRIPT_DIR/pkg"
cp -r "$PROJECT_DIR/alacritty_web/pkg" "$SCRIPT_DIR/pkg"

echo "Done! Serve the demo directory with any HTTP server, e.g.:"
echo "  cd $SCRIPT_DIR && python3 -m http.server 8080"
