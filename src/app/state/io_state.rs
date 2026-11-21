//! 画像の読み込み・保存・ダイアログのバックグラウンド処理の状態

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Instant;

use image::DynamicImage;

use crate::dpi::Dpi;
use crate::image_load::LoadedImage;
use crate::image_output::OutputFormat;
use crate::trim::TrimInfo;

/// 保存処理の種別
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum SaveKind {
    /// ファイルダイアログからの手動保存
    Manual,
    /// 保存先フォルダへの自動保存
    Auto,
}

/// 画像ファイル I/O ワーカーからの通知
pub(in crate::app) enum IoMsg {
    /// 画像読み込みの完了
    Loaded {
        /// 読み込み要求 ID。古い結果を無視するために使用する
        request_id: u64,
        /// ステータス表示用のファイル名
        file_name: String,
        /// 読み込んだ画像、または失敗時のステータスメッセージ
        result: Result<LoadedImage, String>,
    },
    /// 画像保存の完了
    Saved {
        /// 保存処理の種別
        kind: SaveKind,
        /// 保存先パス、または失敗時のステータスメッセージ
        result: Result<PathBuf, String>,
    },
    /// クリップボードへのコピーの完了
    Copied {
        /// 成功、または失敗時のステータスメッセージ
        result: Result<(), String>,
    },
    /// クリップボードからの貼り付けの完了
    Pasted {
        /// 読み込み要求 ID。古い結果を無視するために使用する
        request_id: u64,
        /// 取得した画像、または失敗時のステータスメッセージ
        result: Result<DynamicImage, String>,
    },
    /// クリップボードからファイルのパスを取得した
    PastedFiles {
        /// 読み込み要求 ID。古い結果を無視するために使用する
        request_id: u64,
        /// コピーされていたファイル・フォルダのパス
        paths: Vec<PathBuf>,
    },
    /// ドロップされたパスの展開・振り分けの完了
    DropScanned {
        /// ドロップ要求 ID。後続のドロップで無効化された結果を無視するために使用する
        request_id: u64,
        /// 振り分けの結果、または失敗時のステータスメッセージ
        result: Result<ScannedDrop, String>,
    },
    /// ファイルダイアログが閉じられた
    DialogClosed(DialogResult),
}

/// ドロップされたパスを展開して振り分けた結果
pub(in crate::app) struct ScannedDrop {
    /// 対応画像ファイル (入力順を維持。フォルダは直下を名前順に展開)
    pub images: Vec<PathBuf>,
    /// 対応外・読み取り失敗として除外した件数
    pub skipped: usize,
    /// フォルダを 1 つ以上含んでいたか
    pub has_dir: bool,
}

/// 手動保存の対象。ダイアログを開いた時点の内容を保持する
pub(in crate::app) struct SaveRequest {
    /// 元画像
    pub original: Arc<DynamicImage>,
    /// 保存するトリミング矩形
    pub info: TrimInfo,
    /// 元画像の ICC プロファイル
    pub icc_profile: Option<Arc<[u8]>>,
    /// 元画像の解像度
    pub dpi: Option<Dpi>,
    /// 拡張子なしで入力された場合に補う拡張子
    pub ext: String,
}

/// ファイルダイアログの結果
///
/// キャンセルされた場合は `None` を持つ
pub(in crate::app) enum DialogResult {
    /// 開く画像の選択
    OpenImage(Option<PathBuf>),
    /// 手動保存の保存先の選択
    SaveImage {
        /// 保存対象
        request: SaveRequest,
        /// 選択された保存先
        path: Option<PathBuf>,
    },
    /// 一括トリミングの対象ファイルの選択
    BatchInputs(Option<Vec<PathBuf>>),
    /// 一括トリミングの保存先フォルダの選択
    BatchOutput {
        /// トリミング対象の画像ファイル
        paths: Vec<PathBuf>,
        /// 呼び出し側で対象外として除外した件数
        skipped: usize,
        /// 選択された保存先フォルダ
        dir: Option<PathBuf>,
    },
    /// 自動保存先フォルダの選択
    AutoSaveDir(Option<PathBuf>),
    /// ダイアログ処理スレッドの panic (ステータスメッセージ)
    Failed(String),
}

/// 画像の読み込み・保存のバックグラウンド処理
///
/// 破棄時は実行中の保存スレッドの完了を一定時間まで待ち、書きかけのファイルが残らないようにする
pub(in crate::app) struct IoState {
    /// ワーカースレッドへ渡す送信側
    pub tx: mpsc::Sender<IoMsg>,
    /// ワーカースレッドからの結果受信チャネル
    pub rx: mpsc::Receiver<IoMsg>,
    /// 読み込み要求 ID。新しい読み込み・貼り付け・クリアで更新する
    pub load_request_id: u64,
    /// 実行中の保存スレッド
    pub save_handles: Vec<JoinHandle<()>>,
    /// ドロップ要求 ID。新しいドロップのたびに更新し、古い展開結果を無視する
    pub drop_request_id: u64,
    /// 読み込み完了時のステータスに追記する補足 (ドロップ時の除外件数など)
    pub load_note: Option<String>,
    /// 直近に自動保存を開始した内容。同じ結果を連番ファイルとして重複保存しないために使う
    pub last_auto_save: Option<AutoSaveKey>,
    /// 自動保存の追記前のステータスと追記後のステータス
    ///
    /// 表示中のステータスが追記後のままなら追記前に戻してから追記し、追記が重ならないようにする
    pub auto_save_status: Option<(String, String)>,
    /// ファイルダイアログを表示中かどうか。同時に 1 つだけ開く
    pub dialog_open: bool,
    /// ファイルダイアログの雛形。親ウィンドウを設定しておき、各ダイアログはこれを複製して使う
    ///
    /// 親を指定するとダイアログがモーダルになり、メインウィンドウの裏に隠れない
    pub dialog_template: rfd::FileDialog,
    /// 終了時の待機期限 (`BatchState` と共有する)
    pub shutdown_deadline: super::SharedDeadline,
}

/// 自動保存の内容を識別するキー
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) struct AutoSaveKey {
    /// 保存元画像の読み込み要求 ID
    pub load_request_id: u64,
    /// 保存したトリミング矩形
    pub info: TrimInfo,
    /// 出力形式
    pub format: OutputFormat,
    /// JPEG のエンコード品質
    pub jpeg_quality: u8,
    /// WebP のエンコード品質
    pub webp_quality: u8,
    /// 保存先ディレクトリ
    pub dir: PathBuf,
}

impl Default for IoState {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            load_request_id: 0,
            drop_request_id: 0,
            save_handles: Vec::new(),
            load_note: None,
            last_auto_save: None,
            auto_save_status: None,
            dialog_open: false,
            dialog_template: rfd::FileDialog::new(),
            shutdown_deadline: super::SharedDeadline::default(),
        }
    }
}

impl IoState {
    /// 進行中の読み込みを打ち切り、古い結果を無視する
    pub(in crate::app) fn abort_load(&mut self) {
        self.load_request_id = self.load_request_id.wrapping_add(1);
        self.load_note = None;
    }

    /// 保存が進行中かどうか
    pub(in crate::app) fn is_saving(&self) -> bool {
        !self.save_handles.is_empty()
    }
}

impl IoState {
    /// 実行中の保存スレッドすべての完了を、共通の期限まで待つ
    ///
    /// # Arguments
    /// * `deadline` - 待つのをやめる時刻
    pub(in crate::app) fn shutdown(&mut self, deadline: Instant) {
        for handle in self.save_handles.drain(..) {
            super::join_with_timeout(handle, deadline);
        }
    }
}

impl Drop for IoState {
    /// 共有された期限まで完了を待つ
    fn drop(&mut self) {
        self.shutdown(super::shutdown_deadline(&self.shutdown_deadline));
    }
}
