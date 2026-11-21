#!/bin/bash
# ========================================================================
# Script Name : generate-third-party-notices.sh
# Description : THIRD_PARTY_NOTICES を cargo-about で生成し、利用者向けドキュメントの素材表示を追記する
# Usage       : generate-third-party-notices.sh [TARGET_TRIPLE]
#               TARGET 未指定時は about.toml の targets を使用
# ========================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=cd-project-root.sh
source "$SCRIPT_DIR/cd-project-root.sh"

TARGET_TRIPLE="${1:-}"

OUTPUT_FILE="$PROJECT_ROOT/THIRD_PARTY_NOTICES"
TEMPLATE="$PROJECT_ROOT/scripts/templates/third-party-notices.hbs"
MANIFEST="$PROJECT_ROOT/Cargo.toml"

if ! command -v cargo-about >/dev/null 2>&1; then
    cat >&2 <<'EOF'
エラー: cargo-about が見つかりません。

以下でインストールしてください:
  cargo install cargo-about --locked --features cli
EOF
    exit 1
fi

if [[ ! -f "$TEMPLATE" ]]; then
    echo "エラー: テンプレートが見つかりません: $TEMPLATE" >&2
    exit 1
fi

echo "THIRD_PARTY_NOTICES を生成しています..."

about_args=(
    about generate "$TEMPLATE"
    --config "$PROJECT_ROOT/about.toml"
    --manifest-path "$MANIFEST"
    --output-file "$OUTPUT_FILE"
    --all-features
    --locked
)

if [[ -n "$TARGET_TRIPLE" ]]; then
    about_args+=(--target "$TARGET_TRIPLE")
    echo "  ターゲット: $TARGET_TRIPLE"
fi

cargo "${about_args[@]}"

# cargo-about は Rust の依存だけを扱うため、利用者向けドキュメント (mdBook の HTML) に含まれる素材の表示を追記する
cat "$PROJECT_ROOT/scripts/templates/user-docs-notices.txt" >>"$OUTPUT_FILE"

echo "生成しました: $OUTPUT_FILE"
