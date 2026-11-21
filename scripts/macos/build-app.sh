#!/bin/bash
# ========================================================================
# Script Name : build-app.sh
# Description : macOS 向け .app バンドル作成スクリプト
# Usage       : ./build-app.sh [--release|-r] [--skip-build]
#
# 前提:
#   - cargo-about (THIRD_PARTY_NOTICES 生成)
#   - macOS (sips / iconutil でアイコン生成)
#
# 出力:
#   target/dist/EdgeCut.app
#   (LICENSE / THIRD_PARTY_NOTICES は Contents/Resources/ に同梱)
# ========================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=../common/cd-project-root.sh
source "$SCRIPT_DIR/../common/cd-project-root.sh"
# shellcheck source=../common/cargo-package-metadata.sh
source "$SCRIPT_DIR/../common/cargo-package-metadata.sh"

RELEASE=false
SKIP_BUILD=false

for arg in "$@"; do
    case $arg in
        --release | -r)
            RELEASE=true
            ;;
        --skip-build)
            SKIP_BUILD=true
            ;;
    esac
done

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "エラー: build-app.sh は macOS 上でのみ実行できます" >&2
    exit 1
fi

trap 'echo ".app バンドルの作成に失敗しました" >&2' ERR

read_cargo_package_metadata
TARGET_TRIPLE="$(detect_host_target_triple)"

DIST_ROOT="$PROJECT_ROOT/target/dist"
APP_NAME="EdgeCut"
APP_BUNDLE="$DIST_ROOT/${APP_NAME}.app"
BINARY_PATH="$PROJECT_ROOT/target/release/$CARGO_BIN_NAME"
ICON_SRC="$PROJECT_ROOT/assets/icons/icon.png"
PLIST_TEMPLATE="$PROJECT_ROOT/scripts/templates/Info.plist.template"

echo "macOS .app バンドルの作成を開始します..."
echo "  ターゲット: $TARGET_TRIPLE"
echo

if [[ "$SKIP_BUILD" != true ]]; then
    if [[ "$RELEASE" == true ]]; then
        echo "リリースビルドを実行中..."
        cargo build --release
    else
        echo "リリースビルドを実行中 (--release 推奨)..."
        cargo build --release
    fi
    echo
fi

if [[ ! -f "$BINARY_PATH" ]]; then
    echo "エラー: バイナリが見つかりません: $BINARY_PATH" >&2
    exit 1
fi

"$SCRIPT_DIR/../common/generate-third-party-notices.sh" "$TARGET_TRIPLE"
echo

MACOS_DIR="$APP_BUNDLE/Contents/MacOS"
RESOURCES_DIR="$APP_BUNDLE/Contents/Resources"

rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

install -m 755 "$BINARY_PATH" "$MACOS_DIR/$CARGO_BIN_NAME"
install -m 644 "$PROJECT_ROOT/LICENSE" "$RESOURCES_DIR/LICENSE"
install -m 644 "$PROJECT_ROOT/THIRD_PARTY_NOTICES" "$RESOURCES_DIR/THIRD_PARTY_NOTICES"

sed \
    -e "s/__VERSION__/${CARGO_PACKAGE_VERSION}/g" \
    -e "s/__BIN_NAME__/${CARGO_BIN_NAME}/g" \
    "$PLIST_TEMPLATE" >"$APP_BUNDLE/Contents/Info.plist"

# .icns 生成 (sips + iconutil)
ICONSET_DIR="$(mktemp -d)"
cleanup() {
    rm -rf "$ICONSET_DIR"
}
trap cleanup EXIT

for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$ICON_SRC" --out "$ICONSET_DIR/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$ICON_SRC" --out "$ICONSET_DIR/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET_DIR" -o "$RESOURCES_DIR/EdgeCut.icns"

mkdir -p "$DIST_ROOT"

echo ".app バンドルの作成が完了しました:"
echo "  $APP_BUNDLE"
echo
echo "起動: open \"$APP_BUNDLE\""
