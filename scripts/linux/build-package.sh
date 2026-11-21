#!/bin/bash
# ========================================================================
# Script Name : build-package.sh
# Description : Linux 向け配布パッケージ (tar.gz) 作成スクリプト
# Usage       : ./build-package.sh [--release|-r] [--skip-build]
#
# 前提:
#   - cargo-about (THIRD_PARTY_NOTICES 生成)
#
# 出力:
#   target/dist/{パッケージ名}-{バージョン}-{ターゲット}.tar.gz
#   展開先構成:
#     edge-cut/EdgeCut          実行ファイル
#     edge-cut/LICENSE
#     edge-cut/THIRD_PARTY_NOTICES
#     edge-cut/edge-cut.desktop
#     edge-cut/install.sh
#     edge-cut/icons/edge-cut.png
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

trap 'echo "パッケージ作成に失敗しました" >&2' ERR

read_cargo_package_metadata
TARGET_TRIPLE="$(detect_host_target_triple)"

DIST_ROOT="$PROJECT_ROOT/target/dist"
STAGE_DIR="$DIST_ROOT/${CARGO_PACKAGE_NAME}-${CARGO_PACKAGE_VERSION}-${TARGET_TRIPLE}"
ARCHIVE_PATH="$DIST_ROOT/${CARGO_PACKAGE_NAME}-${CARGO_PACKAGE_VERSION}-${TARGET_TRIPLE}.tar.gz"
BINARY_PATH="$PROJECT_ROOT/target/release/$CARGO_BIN_NAME"

echo "Linux 配布パッケージの作成を開始します..."
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
    echo "先に cargo build --release を実行してください。" >&2
    exit 1
fi

"$SCRIPT_DIR/../common/generate-third-party-notices.sh" "$TARGET_TRIPLE"
echo

rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/icons"

install -m 755 "$BINARY_PATH" "$STAGE_DIR/$CARGO_BIN_NAME"
install -m 644 "$PROJECT_ROOT/LICENSE" "$STAGE_DIR/LICENSE"
install -m 644 "$PROJECT_ROOT/THIRD_PARTY_NOTICES" "$STAGE_DIR/THIRD_PARTY_NOTICES"
install -m 644 "$PROJECT_ROOT/scripts/templates/edge-cut.desktop" "$STAGE_DIR/edge-cut.desktop"
install -m 644 "$PROJECT_ROOT/assets/icons/icon.png" "$STAGE_DIR/icons/edge-cut.png"

cat >"$STAGE_DIR/install.sh" <<'EOF'
#!/bin/sh
# EdgeCut をユーザーローカルへインストールする (~/.local)
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALL_ROOT="${XDG_DATA_HOME:-$HOME/.local/share}/edge-cut"
BIN_DIR="${XDG_BIN_DIR:-$HOME/.local/bin}"
DESKTOP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICON_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps"

mkdir -p "$INSTALL_ROOT" "$BIN_DIR" "$DESKTOP_DIR" "$ICON_DIR"
install -m 755 "$SCRIPT_DIR/EdgeCut" "$INSTALL_ROOT/EdgeCut"
install -m 644 "$SCRIPT_DIR/LICENSE" "$INSTALL_ROOT/LICENSE"
install -m 644 "$SCRIPT_DIR/THIRD_PARTY_NOTICES" "$INSTALL_ROOT/THIRD_PARTY_NOTICES"
ln -sf "$INSTALL_ROOT/EdgeCut" "$BIN_DIR/edge-cut"
install -m 644 "$SCRIPT_DIR/icons/edge-cut.png" "$ICON_DIR/edge-cut.png"
install -m 644 "$SCRIPT_DIR/edge-cut.desktop" "$DESKTOP_DIR/edge-cut.desktop"

echo "インストールしました:"
echo "  実行ファイル: $BIN_DIR/edge-cut"
echo "  データ:       $INSTALL_ROOT"
EOF
chmod +x "$STAGE_DIR/install.sh"

mkdir -p "$DIST_ROOT"
rm -f "$ARCHIVE_PATH"
tar -czf "$ARCHIVE_PATH" -C "$DIST_ROOT" "$(basename "$STAGE_DIR")"

echo "パッケージの作成が完了しました:"
echo "  $ARCHIVE_PATH"
echo
echo "展開: tar -xzf $(basename "$ARCHIVE_PATH")"
echo "インストール: cd ${CARGO_PACKAGE_NAME}-${CARGO_PACKAGE_VERSION}-${TARGET_TRIPLE} && ./install.sh"
