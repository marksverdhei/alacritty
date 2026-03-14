#!/bin/bash
# Build the WASM package and set up the demo site.
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "Building alacritty_web WASM package..."
cd "$PROJECT_DIR/alacritty_web"
wasm-pack build --target web

echo "Copying pkg to demo/static directory..."
rm -rf "$SCRIPT_DIR/static/pkg"
cp -r "$PROJECT_DIR/alacritty_web/pkg" "$SCRIPT_DIR/static/pkg"

echo "Installing demo dependencies..."
cd "$SCRIPT_DIR"
npm install

echo "Building Svelte demo site..."
npm run build

echo "Done! The built site is in demo/build/"
echo "For development, run: cd demo && npm run dev"
