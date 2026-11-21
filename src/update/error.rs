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
    // 確認 (version API / アセット選定)
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
    /// MSI アセットが無い
    AssetMissing {
        /// リリースタグ
        tag: String,
    },
    /// MSI はあるが CPU 向けアセットが無い
    AssetArchMissing {
        /// CPU アーキテクチャ文字列
        arch: String,
        /// リリースタグ
        tag: String,
    },
    /// アセット名が単一のファイル名として安全に扱えない
    AssetNameInvalid,
    /// アセットに digest / sha256 が無い
    DigestMissing {
        /// アセット名
        asset: String,
    },
    /// version API の HTTP ステータスエラー
    CheckHttp {
        /// HTTP ステータスコード
        status: u16,
    },
    /// version API のネットワーク失敗
    CheckNetwork,
    /// version API の JSON 解釈失敗
    InvalidJson,

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
    /// ダウンロードの認証失敗 (`401`)
    DownloadUnauthorized,
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
    /// 更新用一時フォルダの作成失敗
    WorkDirCreate,
    /// MSI パスに不正な文字
    MsiPathInvalid,
    /// `msiexec` 起動失敗
    MsiLaunch,
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CurrentVersionInvalid { version } => {
                write!(f, "現在のバージョンを解釈できません: {version}")
            }
            Self::LatestVersionInvalid { tag } => {
                write!(f, "最新バージョンを解釈できません: {tag}")
            }
            Self::AssetMissing { tag } => {
                write!(f, "最新リリースに MSI アセットがありません (tag={tag})")
            }
            Self::AssetArchMissing { arch, tag } => write!(
                f,
                "最新リリースにこの CPU ({arch}) 向けの MSI アセットがありません (tag={tag})"
            ),
            Self::AssetNameInvalid => {
                write!(f, "最新リリースのアセット名が不正なため更新できません")
            }
            Self::DigestMissing { asset } => write!(
                f,
                "最新リリースにアセットの完全性情報 (digest) がありません (asset={asset})"
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
            Self::DownloadUnauthorized => {
                write!(f, "更新ファイルのダウンロードが認証に失敗しました")
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
            Self::WorkDirCreate => write!(f, "更新用一時フォルダの作成に失敗しました"),
            Self::MsiPathInvalid => write!(f, "MSI パスに不正な文字が含まれています"),
            Self::MsiLaunch => write!(f, "インストーラーの起動に失敗しました"),
        }
    }
}

impl std::error::Error for UpdateError {}
