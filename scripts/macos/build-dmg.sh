#!/bin/bash
# ========================================================================
# Script Name : build-dmg.sh
# Description : macOS 向け DMG 配布イメージ作成スクリプト
# Usage       : ./build-dmg.sh [--release|-r] [--skip-build] [--skip-app]
#
# 前提:
#   - build-app.sh と同じ
#   - hdiutil (macOS 標準)
#
# 出力:
#   target/dist/{パッケージ名}-{バージョン}-{ターゲット}.dmg
#   (EdgeCut.app・Applications へのリンク・利用者向けドキュメント docs/ を収録)
# ========================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=../common/cd-project-root.sh
source "$SCRIPT_DIR/../common/cd-project-root.sh"
# shellcheck source=../common/cargo-package-metadata.sh
source "$SCRIPT_DIR/../common/cargo-package-metadata.sh"

RELEASE=false
SKIP_BUILD=false
SKIP_APP=false

for arg in "$@"; do
    case $arg in
        --release | -r)
            RELEASE=true
            ;;
        --skip-build)
            SKIP_BUILD=true
            ;;
        --skip-app)
            SKIP_APP=true
            ;;
    esac
done

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "エラー: build-dmg.sh は macOS 上でのみ実行できます" >&2
    exit 1
fi

trap 'echo "DMG の作成に失敗しました" >&2' ERR

read_cargo_package_metadata
TARGET_TRIPLE="$(detect_host_target_triple)"

DIST_ROOT="$PROJECT_ROOT/target/dist"
APP_BUNDLE="$DIST_ROOT/EdgeCut.app"
DMG_PATH="$DIST_ROOT/${CARGO_PACKAGE_NAME}-${CARGO_PACKAGE_VERSION}-${TARGET_TRIPLE}.dmg"
DMG_STAGING="$(mktemp -d)"
cleanup() {
    rm -rf "$DMG_STAGING"
}
trap cleanup EXIT

echo "macOS DMG の作成を開始します..."
echo "  ターゲット: $TARGET_TRIPLE"
echo

if [[ "$SKIP_APP" != true ]]; then
    app_args=()
    [[ "$RELEASE" == true ]] && app_args+=(--release)
    [[ "$SKIP_BUILD" == true ]] && app_args+=(--skip-build)
    "$SCRIPT_DIR/build-app.sh" "${app_args[@]}"
    echo
fi

if [[ ! -d "$APP_BUNDLE" ]]; then
    echo "エラー: .app が見つかりません: $APP_BUNDLE" >&2
    echo "先に ./build-app.sh を実行してください。" >&2
    exit 1
fi

if [[ ! -f "$APP_BUNDLE/Contents/Resources/docs/index.html" ]]; then
    echo "エラー: .app に利用者向けドキュメントがありません: $APP_BUNDLE/Contents/Resources/docs" >&2
    echo "./build-app.sh で .app を作り直してください。" >&2
    exit 1
fi

mkdir -p "$DIST_ROOT"
cp -R "$APP_BUNDLE" "$DMG_STAGING/"
ln -s /Applications "$DMG_STAGING/Applications"
# .app 内の docs/ は Finder から辿りにくいため、DMG の直下にも置く
cp -R "$APP_BUNDLE/Contents/Resources/docs" "$DMG_STAGING/docs"

rm -f "$DMG_PATH"
hdiutil create \
    -volname "EdgeCut" \
    -srcfolder "$DMG_STAGING" \
    -ov \
    -format UDZO \
    "$DMG_PATH" >/dev/null

echo "DMG の作成が完了しました:"
echo "  $DMG_PATH"
