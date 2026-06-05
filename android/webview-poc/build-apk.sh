#!/usr/bin/env bash
# Rebuild the pty-server for arm64-v8a, drop it into jniLibs/, and produce
# a fresh debug APK. Run from anywhere — paths are resolved from the
# script's location.
#
# Required env: ANDROID_HOME pointing at the SDK that contains an `ndk/` dir.
# Tested with NDK 27.2.12479018.

set -euo pipefail

SCRIPT_DIR="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
REPO_ROOT="$( cd "$SCRIPT_DIR/../.." && pwd )"

# Space-separated list of ABIs to build. Override via env: ABIS="arm64-v8a".
# Defaults match abiFilters in app/build.gradle.kts.
ABIS=${ABIS:-"arm64-v8a x86_64"}

rust_target_for() {
  case "$1" in
    arm64-v8a)   echo "aarch64-linux-android aarch64-linux-android" ;;
    armeabi-v7a) echo "armv7-linux-androideabi armv7a-linux-androideabi" ;;
    x86_64)      echo "x86_64-linux-android x86_64-linux-android" ;;
    *) echo "unknown ABI: $1" >&2; return 1 ;;
  esac
}

: "${ANDROID_HOME:?ANDROID_HOME not set}"
NDK_DIR=$(ls -d "$ANDROID_HOME"/ndk/*/ 2>/dev/null | head -1 | sed 's:/$::')
[[ -n "$NDK_DIR" ]] || { echo "no NDK under $ANDROID_HOME/ndk/"; exit 1; }
case "$(uname -s)" in
  Linux)  HOST_TAG=linux-x86_64 ;;
  Darwin) HOST_TAG=darwin-x86_64 ;;   # NDK ships a single fat darwin toolchain
  *) echo "unsupported host: $(uname -s)"; exit 1 ;;
esac
TOOLCHAIN="$NDK_DIR/toolchains/llvm/prebuilt/$HOST_TAG"
[[ -d "$TOOLCHAIN" ]] || { echo "NDK toolchain missing: $TOOLCHAIN"; exit 1; }
API_LEVEL=${API_LEVEL:-24}
AR="$TOOLCHAIN/bin/llvm-ar"
STRIP="$TOOLCHAIN/bin/llvm-strip"

step=1
total=$(($(echo "$ABIS" | wc -w) + 2))
for ABI in $ABIS; do
  read -r RUST_TARGET CLANG_PREFIX < <(rust_target_for "$ABI")
  LINKER="$TOOLCHAIN/bin/${CLANG_PREFIX}${API_LEVEL}-clang"
  [[ -x "$LINKER" ]] || { echo "linker missing: $LINKER"; exit 1; }

  if ! rustup target list --installed 2>/dev/null | grep -qx "$RUST_TARGET"; then
    echo "rust target '$RUST_TARGET' not installed; run:"
    echo "  rustup target add $RUST_TARGET"
    exit 1
  fi

  echo "[$step/$total] cargo build --release --target=$RUST_TARGET -p alacritty_pty_server"
  TARGET_UPPER=$(echo "$RUST_TARGET" | tr 'a-z-' 'A-Z_')
  env \
    "CARGO_TARGET_${TARGET_UPPER}_LINKER=$LINKER" \
    "CC_${RUST_TARGET//-/_}=$LINKER" \
    "AR_${RUST_TARGET//-/_}=$AR" \
    cargo build --release --target "$RUST_TARGET" \
      -p alacritty_pty_server --manifest-path "$REPO_ROOT/Cargo.toml"

  BIN_SRC="$REPO_ROOT/target/$RUST_TARGET/release/alacritty-pty-server"
  JNI_DST="$SCRIPT_DIR/app/src/main/jniLibs/$ABI/libalacritty-pty-server.so"
  mkdir -p "$(dirname "$JNI_DST")"
  cp "$BIN_SRC" "$JNI_DST"
  "$STRIP" "$JNI_DST"
  step=$((step + 1))
done

echo "[$step/$total] copy refreshed wasm bundle into assets/pkg/"
WASM_SRC="$REPO_ROOT/demo/static/pkg"
if [[ -d "$WASM_SRC" ]]; then
  rm -rf "$SCRIPT_DIR/app/src/main/assets/pkg"
  cp -r "$WASM_SRC" "$SCRIPT_DIR/app/src/main/assets/pkg"
else
  echo "  (skipped — $WASM_SRC not present; assuming existing bundle is fresh)"
fi
step=$((step + 1))

echo "[$step/$total] gradlew assembleDebug"
cd "$SCRIPT_DIR"
./gradlew :app:assembleDebug

APK="$SCRIPT_DIR/app/build/outputs/apk/debug/app-debug.apk"
echo
echo "APK: $APK ($(stat --printf='%s' "$APK") bytes)"
