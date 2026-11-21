#!/bin/bash
# ========================================================================
# Script Name : build-user-docs.sh
# Description : 利用者向けドキュメント (docs/user) を mdBook で HTML 化する (source 用)
# Usage       : source build-user-docs.sh && build_user_docs
#               (PROJECT_ROOT が設定済みであること。出力先は USER_DOCS_DIR に入る)
# Requires    : mdBook (版は build-user-docs.ps1 の $UserDocsMdBookVersion に固定), perl
#
# scripts/common/build-user-docs.ps1 (Windows) と同じ手順・同じ出力にする。
# 変更履歴の変換規則やリンク検証を変える場合は両方を揃える
# ========================================================================

# mdBook の版は build-user-docs.ps1 の定数を唯一の定義とし、ここでは読み取るだけにする
read_user_docs_mdbook_version() {
    local ps1="$PROJECT_ROOT/scripts/common/build-user-docs.ps1"
    USER_DOCS_MDBOOK_VERSION="$(
        sed -nE "s/^\\\$script:UserDocsMdBookVersion = '([0-9.]+)'.*/\\1/p" "$ps1"
    )"
    if [[ -z "$USER_DOCS_MDBOOK_VERSION" ]]; then
        echo "エラー: mdBook の版を読み取れませんでした: $ps1" >&2
        return 1
    fi
}

# mdBook が所定の版でインストールされているか確認する
assert_mdbook_installed() {
    local install_hint="  cargo install mdbook --locked --version $USER_DOCS_MDBOOK_VERSION"

    if ! command -v mdbook >/dev/null 2>&1; then
        {
            echo "エラー: mdbook が見つかりません"
            echo
            echo "以下でインストールしてください:"
            echo "$install_hint"
        } >&2
        return 1
    fi

    local version_text actual
    version_text="$(mdbook --version 2>&1)"
    actual="$(grep -oE '[0-9]+\.[0-9]+\.[0-9]+' <<<"$version_text" | head -1)"
    if [[ "$actual" != "$USER_DOCS_MDBOOK_VERSION" ]]; then
        {
            echo "エラー: mdbook の版が異なります (実際: $version_text, 期待: $USER_DOCS_MDBOOK_VERSION)"
            echo
            echo "以下で入れ直してください:"
            echo "$install_hint --force"
        } >&2
        return 1
    fi

    if ! command -v perl >/dev/null 2>&1; then
        echo "エラー: perl が見つかりません (変更履歴の変換・リンク検証に使用)" >&2
        return 1
    fi
}

# CHANGELOG.md を利用者向けの変更履歴ページに整える (規則は ConvertTo-UserDocsChangelog と同じ)
# 引数: 入力 CHANGELOG.md, 出力 changelog.md
convert_user_docs_changelog() {
    perl -CSD -Mutf8 -e '
        local $/;
        open my $in, "<:encoding(UTF-8)", $ARGV[0] or die "エラー: $ARGV[0] を開けません: $!\n";
        my $markdown = <$in>;
        close $in;

        # 先頭の記載規約を除き、最初のバージョン見出しから始める
        my ($title) = $markdown =~ /^(# [^\r\n]*)/m;
        my $first = $markdown =~ /^## \[/m ? $-[0] : -1;
        die "エラー: CHANGELOG.md から見出しとバージョン節を読み取れませんでした\n"
            if !defined $title || $first < 0;
        my $body = "$title\n\n" . substr($markdown, $first);

        # バージョン見出しの参照リンクと、末尾の参照リンク定義を除く
        $body =~ s/^## \[([^\]]+)\]/## $1/mg;
        $body =~ s/^\[[^\]]+\]: \S+[ \t]*(\r?\n|\z)//mg;

        # リンクを本の中から辿れる形に書き換える
        $body =~ s{\[([^\]]+)\]\(([^)\s]+)\)}{
            my ($text, $target) = ($1, $2);
            if ($target =~ /^[a-z][a-z0-9+.-]*:/i || $target =~ /^#/) { "[$text]($target)" }
            elsif ($target =~ m{^docs/user/([^/]+\.md(?:#.*)?)$}) { "[$text]($1)" }
            elsif ($target =~ /^CHANGELOG\.md(#.*)?$/) { "[$text](changelog.md" . ($1 // "") . ")" }
            else { $text }
        }ge;

        # 箇条書きの行だけ、「。」の直後に文が続く箇所で折り返す (行末の `\` は強制改行)
        my @lines = split /\r?\n/, $body, -1;
        for (@lines) {
            if (/^(\s*)[-*] /) {
                my $continuation = "$1  ";
                s/。(?=[^\s)）」』])/。\\\n$continuation/g;
            }
        }

        open my $out, ">:encoding(UTF-8)", $ARGV[1] or die "エラー: $ARGV[1] に書き込めません: $!\n";
        print $out join("\n", @lines);
        close $out;
    ' "$1" "$2"
}

# 出力 HTML の相対リンク (href / src) のリンク先ファイルとアンカーが存在するか検証する
# 引数: 出力ディレクトリ
assert_user_docs_links() {
    perl -CSD -Mutf8 -MFile::Find -MFile::Basename -MFile::Spec -e '
        my $root = $ARGV[0];

        sub read_file {
            my ($path) = @_;
            local $/;
            open my $fh, "<:encoding(UTF-8)", $path or die "エラー: $path を開けません: $!\n";
            my $text = <$fh>;
            close $fh;
            return $text;
        }

        sub html_decode {
            my ($s) = @_;
            $s =~ s/&#x([0-9a-f]+);/chr(hex $1)/gie;
            $s =~ s/&#(\d+);/chr $1/ge;
            my %named = (amp => "&", lt => "<", gt => ">", quot => "\"", apos => "\x27");
            $s =~ s/&(amp|lt|gt|quot|apos);/$named{$1}/g;
            return $s;
        }

        sub uri_unescape {
            my ($s) = @_;
            utf8::encode($s);
            $s =~ s/%([0-9a-f]{2})/chr hex $1/gie;
            utf8::decode($s);
            return $s;
        }

        my %id_cache;
        sub ids_of {
            my ($path) = @_;
            $id_cache{$path} //= { map { html_decode($_) => 1 } read_file($path) =~ /\sid="([^"]+)"/g };
            return $id_cache{$path};
        }

        my @pages;
        find(sub { push @pages, $File::Find::name if -f && /\.html$/ }, $root);

        my @errors;
        for my $page (sort @pages) {
            my $relative_page = File::Spec->abs2rel($page, $root);
            for my $raw (read_file($page) =~ /\s(?:href|src)="([^"]+)"/g) {
                my $link = html_decode($raw);
                next if $link =~ /^[a-z][a-z0-9+.-]*:/i || $link =~ m{^//};

                my ($path_part, $fragment) = split /#/, $link, 2;
                $path_part = uri_unescape((split /\?/, $path_part // "", 2)[0] // "");
                $fragment = defined $fragment ? uri_unescape($fragment) : "";

                my $target = $path_part ne "" ? File::Spec->catfile(dirname($page), $path_part) : $page;
                if (!-f $target) {
                    push @errors, "$relative_page -> $link (ファイルが無い)";
                    next;
                }
                if ($fragment ne "" && $target =~ /\.html$/ && !ids_of($target)->{$fragment}) {
                    push @errors, "$relative_page -> $link (アンカーが無い)";
                }
            }
        }

        if (@errors) {
            print STDERR "エラー: 利用者向けドキュメントにリンク切れが " . scalar(@errors) . " 件あります\n";
            print STDERR "  $_\n" for @errors;
            exit 1;
        }
    ' "$1"
}

# 出力に同梱される第三者素材が、THIRD_PARTY_NOTICES に追記する表示と合っているか検証する
# (規則は Assert-UserDocsAssets と同じ)
# 引数: 出力ディレクトリ
assert_user_docs_assets() {
    perl -CSD -Mutf8 -MFile::Find -e '
        my ($root, $notices_path, $mdbook_version) = @ARGV;

        my (@fonts, @texts);
        find(sub {
            return unless -f;
            push @fonts, $_ if /\.(?:woff2?|ttf|otf)$/i;
            if (/\.(?:html|js|css|svg)$/i) {
                local $/;
                open my $fh, "<:encoding(UTF-8)", $_ or die "エラー: $File::Find::name を開けません: $!\n";
                push @texts, scalar <$fh>;
                close $fh;
            }
        }, $root);
        die "エラー: 利用者向けドキュメントにフォントが含まれています ($fonts[0] など)。"
            . "docs/user/theme/fonts/fonts.css を確認してください\n" if @fonts;

        my $notices = do {
            local $/;
            open my $fh, "<:encoding(UTF-8)", $notices_path or die "エラー: $notices_path を開けません: $!\n";
            <$fh>;
        };

        my %expected = ("mdBook $mdbook_version" => 1);
        my @patterns = (
            [qr/Highlight\.js (\d+\.\d+\.\d+)/, "highlight.js"],
            [qr/clipboard\.js v(\d+\.\d+\.\d+)/, "clipboard.js"],
            [qr/Font Awesome Free (\d+\.\d+\.\d+)/, "Font Awesome Free"],
        );
        for my $text (@texts) {
            for my $p (@patterns) {
                my ($re, $label) = @$p;
                $expected{"$label $1"} = 1 while $text =~ /$re/g;
            }
        }

        my @missing = grep { index($notices, $_) < 0 } sort keys %expected;
        if (@missing) {
            print STDERR "エラー: 利用者向けドキュメントの第三者素材の表示が出力と合っていません\n";
            print STDERR "  $notices_path に記載がない: $_\n" for @missing;
            print STDERR "mdBook の版を変えた場合は、同梱素材とライセンスを確認して表示を更新してください\n";
            exit 1;
        }
    ' "$1" "$PROJECT_ROOT/scripts/templates/user-docs-notices.txt" "$USER_DOCS_MDBOOK_VERSION"
}

# docs/user を mdBook でビルドし、出力ディレクトリ (target/user-docs) を USER_DOCS_DIR に設定する
# CHANGELOG.md のリンクを書き換えるため、原稿を target/user-docs-src へ複製してからビルドする
build_user_docs() {
    read_user_docs_mdbook_version
    assert_mdbook_installed

    local source_dir="$PROJECT_ROOT/docs/user"
    local staging_dir="$PROJECT_ROOT/target/user-docs-src"
    local output_dir="$PROJECT_ROOT/target/user-docs"

    echo "利用者向けドキュメントを生成しています (mdBook)..."

    rm -rf "$staging_dir" "$output_dir"
    mkdir -p "$staging_dir"
    cp -R "$source_dir/." "$staging_dir/"

    convert_user_docs_changelog "$PROJECT_ROOT/CHANGELOG.md" "$staging_dir/changelog.md"

    mdbook build "$staging_dir" --dest-dir "$output_dir"

    # src = "." のため、book.toml・theme/ (テーマの上書き)・custom.css (ハッシュ付きの複製を参照する) が
    # 原稿のまま出力へ複製される。404.html (<base href="/">) と .nojekyll は Web 公開用で、
    # ローカルのファイルとして開く配布物では使わない
    rm -rf "$output_dir/book.toml" "$output_dir/theme" "$output_dir/custom.css" \
        "$output_dir/404.html" "$output_dir/.nojekyll"

    assert_user_docs_links "$output_dir"
    assert_user_docs_assets "$output_dir"

    local file_count
    file_count="$(find "$output_dir" -type f | wc -l | tr -d ' ')"
    echo "  出力: $output_dir ($file_count ファイル)"

    USER_DOCS_DIR="$output_dir"
}
