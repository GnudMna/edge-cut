# リリース

## release.ps1

`release/v{version}` ブランチ上で `scripts/windows/release.ps1` を実行する。

```powershell
git checkout -b release/v0.3.0
./scripts/windows/release.ps1 -Version 0.3.0   # または -Bump Patch。-DryRun で変更内容のみ表示、-Date でリリース日を指定 (既定は当日)
```

1. `Cargo.toml` / `Cargo.lock` のバージョンを更新し、`CHANGELOG.md` の `[Unreleased]` 節をリリース見出しへ移す (空だと失敗する)
2. `check.ps1` で品質チェックを実行し (`-SkipCheck` で省略)、`Release: v{version}` でコミットする (`-NoCommit` で省略)
3. `build-msi.ps1` で MSI を作成して内容を検証し ([MSI の検証](setup.md#msi-の検証))、`build-portable.ps1` でポータブル版の ZIP (`target\portable\edge-cut-{version}-x86_64-portable.zip`) を作成し、`minisign` で `<アセット>.minisig` を作って `keys/update-signing/*.pub` で検証する (`-SkipMsi` でいずれも省略)

**前提:**

- 環境変数 `EDGE_CUT_SIGNING_KEY` (minisign 秘密鍵のパス) を設定しておく。署名の準備はビルド前に確認する
- MSI 作成には [開発環境とビルド](setup.md#前提条件) と同じ前提 (cargo-wix・cargo-about・mdBook・WiX Toolset v3) が必要。mdBook の未導入や版違いはビルド前に検出して止まる
- 既定では作業ツリーがクリーンであること (`-AllowDirty` で上書き可)

**手動で残る作業:** `release/v{version}` を push し、Gitea 上で `v{version}` タグを付与する。GitHub Release へ MSI・ポータブル版の ZIP と署名ファイル (`*.msi.minisig` / `*-portable.zip.minisig`) をアップロードして公開し (ドラフト・プレリリースはアプリの更新対象にならない)、次を確認する。

- `https://api.github.com/repos/GnudMna/edge-cut/releases/latest` がこの版を返し、MSI と ZIP に `digest` (`sha256:<hex>`) がある (MSI 版は `.msi`、ポータブル版は `-portable.zip` のアセットで更新する)
- 署名ファイルをアセットと同じ URL 形式 (`https://github.com/GnudMna/edge-cut/releases/download/{tag}/{asset}.minisig`) で取得できる
- 2.0.0 以前のアプリは gh-release-bridge からしか更新できない。bridge を止めるまでは、Worker Secret `EDGE_CUT_KEY` を変えない (配布済みのアプリに同じ値が埋め込まれている)

## 更新署名鍵

アプリ更新は、GitHub API の digest (SHA-256) に加えて MSI・ZIP の minisign (Ed25519) 署名を検証してから、更新プログラムがインストール (MSI は `msiexec /qn`、ポータブル版はファイルの差し替え) する。署名の信頼コメント (`edge-cut-update version={version} file={アセット名}`) も照合するため、配信経路や digest を差し替えられても、正規の秘密鍵で署名されていない MSI や別バージョン向けの署名済み MSI は適用しない。

公開鍵は `keys/update-signing/*.pub` に置き、`build.rs` がビルド時にすべて埋め込む。秘密鍵はリポジトリに置かない。`src/update/signature.rs` の信頼コメント形式は `scripts/common/update-signing.ps1` と揃える。

```powershell
# minisign の導入
winget install jedisct1.minisign

# 鍵ペアの生成 (秘密鍵はリポジトリ外。パスワード保護を推奨)
minisign -G -p keys/update-signing/edge-cut-release.pub -s "$env:USERPROFILE\.minisign\edge-cut-release.key"

# release.ps1 で使う秘密鍵のパス
$env:EDGE_CUT_SIGNING_KEY = "$env:USERPROFILE\.minisign\edge-cut-release.key"
```

- 公開鍵を追加・変更した版を配布してから、その鍵で署名した更新を出す。利用者の手元のアプリは自分に埋め込まれた公開鍵でしか検証できない
- 鍵の入れ替えは、新旧の公開鍵を両方置いた版を一度配布し、以降の版で旧鍵を外す。秘密鍵を紛失・漏洩した場合も同じ手順で、旧鍵を外した版は手動で配布する
- 公開鍵が 1 つも無いビルドは更新を適用しない (開発ビルドでの誤適用を防ぐ)。`release.ps1` は公開鍵が無いと失敗する
