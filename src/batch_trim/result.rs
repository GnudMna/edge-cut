//! 一括トリミングの設定・進捗・集計結果

use std::path::PathBuf;

use crate::image_output::{OutputFormat, SaveOptions};

// ======================================================================
// パラメータ・結果
// ======================================================================
/// 一括トリミングのトリム設定
#[derive(Debug, Clone, Copy)]
pub struct BatchTrimParams {
    /// 余白検出の許容度 (0–100)
    pub tolerance: u32,
    /// トリミング後に残す余白ピクセル数
    pub padding: u32,
    /// 手動指定の背景色。`None` のとき四辺から自動検出する
    pub bg_override: Option<[u8; 4]>,
    /// 出力形式
    pub format: OutputFormat,
    /// 保存時のエンコード設定
    pub save_options: SaveOptions,
}

/// バックグラウンド一括処理の進捗通知
#[derive(Debug, Clone)]
pub enum BatchProgress {
    /// 処理中 (`processed` は完了したファイル数)
    Working {
        /// 処理済みファイル数
        processed: usize,
        /// 対象ファイル総数
        total: usize,
    },
    /// 完了
    Finished(BatchTrimResult),
}

/// 一括トリミングの集計結果
#[derive(Debug, Clone)]
pub struct BatchTrimResult {
    /// 正常に保存したファイル数
    pub saved: usize,
    /// 連番サフィックスで衝突回避した保存数
    pub renamed: usize,
    /// 失敗したファイルごとのメッセージ (`{ファイル名}: {理由}`)
    pub errors: Vec<String>,
    /// ユーザーにより途中キャンセルされたか
    pub cancelled: bool,
    /// 対象ファイル総数
    pub total: usize,
    /// 出力先ディレクトリ
    pub out_dir: PathBuf,
    /// 出力フォルダ作成など、バッチ全体の失敗メッセージ
    pub fatal_error: Option<String>,
}

impl BatchTrimResult {
    /// 出力フォルダ作成失敗など、処理開始前の致命的エラーの結果を生成する
    ///
    /// # Arguments
    /// * `out_dir` - 出力先ディレクトリ
    /// * `message` - ステータスバーに表示する失敗メッセージ
    ///
    /// # Returns
    /// * `Self` - 処理件数が 0 で `fatal_error` を設定した結果
    #[must_use]
    pub fn fatal(out_dir: PathBuf, message: impl Into<String>) -> Self {
        Self {
            saved: 0,
            renamed: 0,
            errors: Vec::new(),
            cancelled: false,
            total: 0,
            out_dir,
            fatal_error: Some(message.into()),
        }
    }

    /// ステータスバー向けの要約メッセージ (1 行) を生成する
    ///
    /// # Arguments
    /// * `self` - 一括処理の結果
    ///
    /// # Returns
    /// * `String` - 致命的エラーがあればそのメッセージ、なければ件数と出力先の要約
    #[must_use]
    pub fn summary(&self) -> String {
        if let Some(msg) = &self.fatal_error {
            return msg.clone();
        }

        let dest = self.out_dir.display();
        if self.cancelled {
            let renamed_note = if self.renamed > 0 {
                format!("、{renamed} 件は連番回避", renamed = self.renamed)
            } else {
                String::new()
            };
            return format!(
                "一括トリミングをキャンセルしました ({saved}/{total} 保存済み{renamed_note}) → {dest}",
                saved = self.saved,
                total = self.total,
                renamed_note = renamed_note,
            );
        }

        let error_count = self.errors.len();
        if error_count == 0 {
            let renamed_note = if self.renamed > 0 {
                format!(" ({renamed} 件は連番回避)", renamed = self.renamed)
            } else {
                String::new()
            };
            return format!(
                "一括トリミング完了: {saved}/{total} ファイルを保存{renamed_note} → {dest}",
                saved = self.saved,
                total = self.total,
                renamed_note = renamed_note,
            );
        }

        let first = self.errors.first().map_or("", String::as_str);
        if error_count == 1 {
            format!(
                "一括トリミング完了: {saved}/{total} 成功、1 件エラー → {dest} ({first})",
                saved = self.saved,
                total = self.total,
            )
        } else {
            format!(
                "一括トリミング完了: {saved}/{total} 成功、{error_count} 件エラー → {dest} (先頭: {first})",
                saved = self.saved,
                total = self.total,
            )
        }
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// `summary()` がエラー件数を 1 行に要約すること
    #[test]
    fn summary_omits_full_error_list() {
        let result = BatchTrimResult {
            saved: 1,
            renamed: 0,
            errors: vec!["a.png: err1".into(), "b.png: err2".into()],
            cancelled: false,
            total: 3,
            out_dir: PathBuf::from("/out"),
            fatal_error: None,
        };
        let msg = result.summary();
        assert!(msg.contains("2 件エラー"));
        assert!(msg.contains("先頭: a.png: err1"));
        assert!(!msg.contains("b.png: err2"));
    }
}
