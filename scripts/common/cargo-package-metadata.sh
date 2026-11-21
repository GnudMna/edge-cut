#!/bin/bash
# ========================================================================
# Script Name : cargo-package-metadata.sh
# Description : Cargo.toml からパッケージ名・バージョン・バイナリ名を読み取る
# Usage       : source cargo-package-metadata.sh && read_cargo_package_metadata
# ========================================================================

# Cargo.toml から name / version / [[bin]].name を取得する
read_cargo_package_metadata() {
    local manifest="${PROJECT_ROOT:-.}/Cargo.toml"

    if [[ ! -f "$manifest" ]]; then
        echo "エラー: Cargo.toml が見つかりません: $manifest" >&2
        return 1
    fi

    CARGO_PACKAGE_NAME="$(
        grep -E '^name[[:space:]]*=' "$manifest" | head -1 \
            | sed -E 's/^name[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/'
    )"
    CARGO_PACKAGE_VERSION="$(
        grep -E '^version[[:space:]]*=' "$manifest" | head -1 \
            | sed -E 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/'
    )"
    CARGO_BIN_NAME="$(
        awk '
            /^\[\[bin\]\]/ { in_bin = 1; next }
            /^\[/ { in_bin = 0 }
            in_bin && /^name[[:space:]]*=/ {
                sub(/^name[[:space:]]*=[[:space:]]*"/, "")
                sub(/".*$/, "")
                print
                exit
            }
        ' "$manifest"
    )"

    if [[ -z "$CARGO_PACKAGE_NAME" || -z "$CARGO_PACKAGE_VERSION" || -z "$CARGO_BIN_NAME" ]]; then
        echo "エラー: Cargo.toml からメタデータを読み取れませんでした" >&2
        return 1
    fi
}

# ホストの CPU アーキテクチャから cargo --target 用 triple を返す
detect_host_target_triple() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)
            case "$arch" in
                x86_64) echo "x86_64-unknown-linux-gnu" ;;
                aarch64 | arm64) echo "aarch64-unknown-linux-gnu" ;;
                *)
                    echo "エラー: 未対応の Linux アーキテクチャ: $arch" >&2
                    return 1
                    ;;
            esac
            ;;
        Darwin)
            case "$arch" in
                arm64) echo "aarch64-apple-darwin" ;;
                x86_64) echo "x86_64-apple-darwin" ;;
                *)
                    echo "エラー: 未対応の macOS アーキテクチャ: $arch" >&2
                    return 1
                    ;;
            esac
            ;;
        *)
            echo "エラー: 未対応の OS: $os" >&2
            return 1
            ;;
    esac
}
