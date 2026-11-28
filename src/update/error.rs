//! アプリ更新の型付きエラー
//!
//! ユーザー向け文言は `Display` に集約し、Outcome / ステータスへは `to_string()` で渡す

use std::fmt;

// ======================================================================
// エラー型
// ======================================================================
/// 更新確認・ダウンロード・適用の失敗
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpdateError {
    // ----------------------------------------------------------------------
    // 確認 (GitHub API / アセット選定)
    // ----------------------------------------------------------------------
    /// 現在バージョンの `SemVer` 解釈失敗
    CurrentVersionInvalid {
        /// 解釈できなかった文字列
        version: String,
    },
    /// 最新タグの `SemVer` 解釈失敗
    LatestVersionInvalid {
        /// 解釈できなかったタグ
        tag: String,
    },
    /// 配布物の種類に合うアセットが無い
    AssetMissing {
        /// 配布物の種類 (`MSI` など)
        kind: &'static str,
        /// リリースタグ
        tag: String,
    },
    /// 種類の合うアセットはあるが CPU 向けアセットが無い
    AssetArchMissing {
        /// 配布物の種類 (`MSI` など)
        kind: &'static str,
        /// CPU アーキテクチャ文字列
        arch: String,
        /// リリースタグ
        tag: String,
    },
    /// アセット名が単一のファイル名として安全に扱えない
    AssetNameInvalid,
    /// アセットに digest が無い
    DigestMissing {
        /// アセット名
        asset: String,
    },
    /// GitHub API の回数制限などによる拒否 (`403` / `429`)
    CheckRateLimited {
        /// HTTP ステータスコード
        status: u16,
    },
    /// GitHub API の HTTP ステータスエラー
    CheckHttp {
        /// HTTP ステータスコード
        status: u16,
    },
    /// GitHub API のネットワーク失敗
    CheckNetwork,
    /// GitHub API の JSON 解釈失敗
    InvalidJson,
    /// 更新サーバーとの通信のタイムアウト (確認・署名取得・ダウンロード共通)
    NetworkTimeout,

    // ----------------------------------------------------------------------
    // 完全性 / ダウンロード
    // ----------------------------------------------------------------------
    /// digest が空
    DigestEmpty,
    /// digest の形式が不正
    DigestInvalid {
        /// 生の digest 文字列
        raw: String,
    },
    /// 宣言サイズが 0
    SizeZero,
    /// 宣言サイズが絶対上限を超過
    SizeExceedsMax {
        /// 絶対上限バイト数
        max: u64,
    },
    /// ダウンロードの HTTP ステータスエラー
    DownloadHttp {
        /// HTTP ステータスコード
        status: u16,
    },
    /// ダウンロードの一般失敗 (読み取り含む)
    DownloadFailed,
    /// 保存先への書き込み失敗
    SaveFailed,
    /// サイズ不一致 (実測値が分かっている)
    SizeMismatch {
        /// 期待バイト数
        expected: u64,
        /// 実際のバイト数
        actual: u64,
    },
    /// サイズ上限超過で打ち切り (累積オーバーフロー)
    SizeCapOverflow,
    /// サイズ上限超過で打ち切り (実測が期待より大きい)
    SizeMismatchTruncated {
        /// 期待バイト数 (打ち切り上限)
        expected: u64,
    },
    /// SHA-256 不一致
    IntegrityFailed,

    // ----------------------------------------------------------------------
    // 署名
    // ----------------------------------------------------------------------
    /// 信頼する公開鍵がビルドに埋め込まれていない
    SignatureKeyMissing,
    /// 署名ファイル取得の HTTP ステータスエラー
    SignatureDownloadHttp {
        /// HTTP ステータスコード
        status: u16,
    },
    /// 署名ファイル取得の一般失敗 (読み取り・サイズ超過含む)
    SignatureDownloadFailed,
    /// 署名の形式不正、または内容と一致しない
    SignatureInvalid,
    /// 署名鍵が信頼する公開鍵に含まれない
    SignatureUntrustedKey,
    /// 信頼コメントの更新先バージョン・アセット名が一致しない
    SignatureMetadataMismatch,

    // ----------------------------------------------------------------------
    // 適用
    // ----------------------------------------------------------------------
    /// 利用者がダウンロードを中止した
    Canceled,
    /// 更新用一時フォルダの作成失敗
    WorkDirCreate,
    /// ZIP を開けない・形式が不正
    ZipInvalid,
    /// ZIP の展開失敗
    ZipExtract,
    /// 展開した配布物に必要なファイルが無い
    PortableInvalid,

    // ----------------------------------------------------------------------
    // インストール
    // ----------------------------------------------------------------------
    /// 実行フォルダが MSI の導入先 (`<インストール先>in`) ではない
    InstallLocationMismatch,
    /// 差し替え先・差し替え元のフォルダが正しくない
    InstallDirInvalid,
    /// パスに引数として扱えない文字が含まれる
    PathInvalid,
    /// `msiexec` 起動失敗
    MsiLaunch,
    /// 別のインストールが進行中 (`msiexec` 終了コード 1618)
    MsiBusy,
    /// `msiexec` が失敗した
    MsiFailed {
        /// 終了コード
        code: i32,
    },
    /// ポータブル版のファイルの差し替えに失敗した (差し替え前の状態へ戻した)
    ReplaceFailed {
        /// 失敗したファイルと理由
        detail: String,
    },
    /// ポータブル版の差し替えに失敗し、差し替え前の状態へ戻せなかった
    RollbackFailed {
        /// 退避したファイルの場所
        backup: String,
    },
    /// `EdgeCut` の終了を待ちきれなかった
    ExitTimeout,
    /// 新しい版の起動失敗
    RestartFailed {
        /// 理由
        detail: String,
    },

    // ----------------------------------------------------------------------
    // 更新プログラムの起動 (本体側)
    // ----------------------------------------------------------------------
    /// 実行フォルダに更新プログラムが無い
    UpdaterMissing,
    /// 更新プログラムのコピー・起動に失敗した
    UpdaterLaunch,
    /// 更新処理の内部エラー (panic など)
    Internal,
}

impl fmt::Display for UpdateError {
    #[expect(
        clippy::too_many_lines,
        reason = "利用者向けの文言を 1 か所に並べて見比べられるようにする"
    )]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CurrentVersionInvalid { version } => {
                write!(f, "現在のバージョンを解釈できません: {version}")
            }
            Self::LatestVersionInvalid { tag } => {
                write!(f, "最新バージョンを解釈できません: {tag}")
            }
            Self::AssetMissing { kind, tag } => {
                write!(
                    f,
                    "最新リリースに {kind} のアセットがありません (tag={tag})"
                )
            }
            Self::AssetArchMissing { kind, arch, tag } => write!(
                f,
                "最新リリースにこの CPU ({arch}) 向けの {kind} のアセットがありません (tag={tag})"
            ),
            Self::AssetNameInvalid => {
                write!(f, "最新リリースのアセット名が不正なため更新できません")
            }
            Self::DigestMissing { asset } => write!(
                f,
                "最新リリースにアセットの完全性情報 (digest) がありません (asset={asset})"
            ),
            Self::CheckRateLimited { status } => write!(
                f,
                "更新情報の取得が拒否されました (HTTP {status})。時間をおいて再実行してください"
            ),
            Self::CheckHttp { status } => {
                write!(f, "更新情報の取得に失敗しました (HTTP {status})")
            }
            Self::CheckNetwork => {
                write!(
                    f,
                    "更新情報の取得に失敗しました。ネットワーク接続を確認してください"
                )
            }
            Self::InvalidJson => write!(f, "更新情報の形式が不正です"),
            Self::NetworkTimeout => write!(
                f,
                "更新サーバーとの通信がタイムアウトしました。ネットワーク接続を確認してください"
            ),
            Self::DigestEmpty => write!(f, "アセットの完全性情報 (digest) が空です"),
            Self::DigestInvalid { raw } => {
                write!(f, "アセットの完全性情報 (digest) の形式が不正です: {raw}")
            }
            Self::SizeZero => write!(f, "更新アセットのサイズ情報が不正です (0 バイト)"),
            Self::SizeExceedsMax { max } => {
                write!(
                    f,
                    "更新アセットのサイズが上限 ({max} バイト) を超えています"
                )
            }
            Self::DownloadHttp { status } => {
                write!(
                    f,
                    "更新ファイルのダウンロードに失敗しました (HTTP {status})"
                )
            }
            Self::DownloadFailed => write!(f, "更新ファイルのダウンロードに失敗しました"),
            Self::SaveFailed => write!(f, "更新ファイルの保存に失敗しました"),
            Self::SizeMismatch { expected, actual } => write!(
                f,
                "更新ファイルのサイズが一致しません (期待 {expected} / 実際 {actual})"
            ),
            Self::SizeCapOverflow => {
                write!(
                    f,
                    "更新ファイルのサイズが上限を超えたためダウンロードを中断しました"
                )
            }
            Self::SizeMismatchTruncated { expected } => write!(
                f,
                "更新ファイルのサイズが一致しません (期待 {expected} / 実際はそれ以上)"
            ),
            Self::IntegrityFailed => write!(
                f,
                "更新ファイルの完全性検証 (SHA-256) に失敗しました。再ダウンロードしてください"
            ),
            Self::SignatureKeyMissing => write!(
                f,
                "このビルドには更新の署名検証用の公開鍵が含まれていないため、更新を適用できません"
            ),
            Self::SignatureDownloadHttp { status } => {
                write!(f, "更新ファイルの署名の取得に失敗しました (HTTP {status})")
            }
            Self::SignatureDownloadFailed => write!(f, "更新ファイルの署名の取得に失敗しました"),
            Self::SignatureInvalid => write!(
                f,
                "更新ファイルの署名検証に失敗しました。改ざんされている可能性があります"
            ),
            Self::SignatureUntrustedKey => {
                write!(f, "更新ファイルが信頼できない鍵で署名されています")
            }
            Self::SignatureMetadataMismatch => write!(
                f,
                "更新ファイルの署名が対象のバージョン・ファイルと一致しません"
            ),
            Self::Canceled => write!(f, "更新を中止しました"),
            Self::WorkDirCreate => write!(f, "更新用一時フォルダの作成に失敗しました"),
            Self::ZipInvalid => write!(f, "更新ファイル (ZIP) を開けません"),
            Self::ZipExtract => write!(f, "更新ファイル (ZIP) の展開に失敗しました"),
            Self::PortableInvalid => {
                write!(f, "更新ファイルにポータブル版の EdgeCut が含まれていません")
            }
            Self::InstallLocationMismatch => write!(
                f,
                "インストールした場所 (bin フォルダ) の EdgeCut ではないため、更新できません"
            ),
            Self::InstallDirInvalid => {
                write!(f, "更新先のフォルダが正しくないため、更新できません")
            }
            Self::PathInvalid => write!(f, "パスに使えない文字が含まれています"),
            Self::MsiLaunch => write!(f, "インストーラーの起動に失敗しました"),
            Self::MsiBusy => write!(
                f,
                "別のインストールが進行中です。完了してから再実行してください"
            ),
            Self::MsiFailed { code } => {
                write!(f, "インストールに失敗しました (終了コード {code})")
            }
            Self::ReplaceFailed { detail } => write!(
                f,
                "ファイルの差し替えに失敗したため、更新前の状態へ戻しました: {detail}"
            ),
            Self::RollbackFailed { backup } => write!(
                f,
                "ファイルの差し替えに失敗し、更新前の状態へ戻せませんでした。元のファイルは {backup} にあります"
            ),
            Self::ExitTimeout => write!(f, "EdgeCut が終了しないため、更新を中止しました"),
            Self::RestartFailed { detail } => {
                write!(f, "新しいバージョンの EdgeCut を起動できません: {detail}")
            }
            Self::UpdaterMissing => write!(
                f,
                "更新プログラム (EdgeCutUpdater.exe) が見つかりません。配布物を入れ直してください"
            ),
            Self::UpdaterLaunch => write!(f, "更新プログラムの起動に失敗しました"),
            Self::Internal => write!(f, "更新処理が異常終了しました"),
        }
    }
}

impl std::error::Error for UpdateError {}
