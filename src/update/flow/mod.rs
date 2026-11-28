//! 更新プログラムの進行 (確認 → ダウンロード → 本体の終了待ち → インストール → 起動)
//!
//! 画面と副作用はトレイト (`Ui` / `Ops`) で受け取り、進め方と成否の判定をここに集める。
//! 本体の終了を依頼するまでは中止でき、本体はそれまで動作したまま

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::error::UpdateError;
use super::types::{AvailableUpdate, InstallOutcome, PreparedPackage, UpdateCheckOutcome};

// ======================================================================
// 定数
// ======================================================================
/// 本体の終了を待つ上限 (保存中の処理の完了を待つ時間を含む)
pub(crate) const EXIT_TIMEOUT: Duration = Duration::from_mins(5);
/// ダウンロードの進捗を画面へ送る刻み (全体に対する割合の逆数)
const PROGRESS_STEPS: u64 = 200;

// ======================================================================
// 画面
// ======================================================================
/// 更新プログラムの画面
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Page {
    /// 更新を確認中
    Checking,
    /// 最新版を使用中
    UpToDate {
        /// 現在のバージョン
        current: String,
    },
    /// 確認に失敗
    CheckFailed {
        /// 理由
        reason: String,
    },
    /// 更新がある (利用者の操作を待つ)
    Available {
        /// 現在のバージョン
        current: String,
        /// 更新先のバージョン
        latest: String,
        /// ダウンロードサイズ (バイト)
        size: u64,
    },
    /// ダウンロード・検証中 (中止できる)
    Downloading {
        /// 更新先のバージョン
        latest: String,
        /// 受信済みバイト数
        received: u64,
        /// 全体のバイト数
        total: u64,
    },
    /// 本体の終了待ち
    WaitingExit,
    /// インストール中
    Installing {
        /// 更新先のバージョン
        latest: String,
        /// 完了後に本体を起動し直すか (利用者が本体を閉じていた場合は起動しない)
        restart: bool,
    },
    /// インストールが終わった (利用者が本体を閉じていたため起動していない)
    Installed {
        /// 更新先のバージョン
        latest: String,
    },
    /// 新しい版を起動中
    Starting,
    /// インストールは終わったが Windows の再起動が必要
    RebootRequired {
        /// 更新先のバージョン
        latest: String,
    },
    /// 利用者が中止した (本体は動作したまま)
    Canceled,
    /// 失敗した
    Failed {
        /// 理由
        reason: String,
        /// 詳細ログ (`msiexec` のログがある場合)
        log: Option<PathBuf>,
        /// 本体を終了させた後の失敗で、起動し直せるか
        can_launch: bool,
    },
}

impl Page {
    /// 中止を受け付けるページか (ウィンドウを閉じる操作を中止として扱う)
    ///
    /// # Arguments
    /// * `self` - ページ
    ///
    /// # Returns
    /// * `bool` - ダウンロード中なら `true`
    pub(crate) fn is_cancelable(&self) -> bool {
        matches!(self, Self::Downloading { .. })
    }
}

/// 本体が終了した経緯
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppStop {
    /// 終了を依頼して終了した (更新後に起動し直す)
    Requested,
    /// 依頼する前に利用者が閉じていた (更新後も起動しない)
    ClosedByUser,
}

/// 利用者の操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    /// 更新する
    Update,
    /// 閉じる
    Close,
    /// `EdgeCut` を起動して閉じる
    LaunchApp,
}

// ======================================================================
// 画面と副作用
// ======================================================================
/// 更新プログラムの画面
pub(crate) trait Ui {
    /// ページを表示する (操作を待たない)
    fn show(&self, page: Page);
    /// ページを表示し、利用者の操作を待つ
    fn ask(&self, page: Page) -> Action;
    /// 利用者が中止を求めたか
    fn cancel_requested(&self) -> bool;
}

/// 更新の副作用
pub(crate) trait Ops {
    /// 最新リリースを確認する
    fn check(&self) -> UpdateCheckOutcome;
    /// このインストール先を更新できるか確かめる
    fn ensure_updatable(&mut self) -> Result<(), UpdateError>;
    /// ダウンロード・検証・展開する (`on_progress` が `false` を返すと中止する)
    fn prepare(
        &self,
        update: &AvailableUpdate,
        on_progress: &mut dyn FnMut(u64) -> bool,
    ) -> Result<PreparedPackage, UpdateError>;
    /// 本体へ終了を依頼し、終了するまで待つ (既に閉じられていれば依頼せずに終了を確かめる)
    fn stop_app(&self, timeout: Duration) -> Result<AppStop, UpdateError>;
    /// インストールする
    fn install(&mut self, package: &PreparedPackage) -> Result<InstallOutcome, UpdateError>;
    /// 本体を起動する
    fn start(&self) -> Result<(), UpdateError>;
    /// 失敗時に案内する詳細ログ
    fn log_path(&self) -> Option<PathBuf>;
    /// ダウンロードしたファイルを片付ける
    fn cleanup(&self, update: &AvailableUpdate);
}

// ======================================================================
// 進行
// ======================================================================
/// 更新を最後まで進め、利用者が結果を閉じるか新しい版を起動したら戻る
///
/// # Arguments
/// * `ui` - 画面
/// * `ops` - 副作用
pub(crate) fn run_flow(ui: &impl Ui, ops: &mut impl Ops) {
    ui.show(Page::Checking);
    let update = match ops.check() {
        UpdateCheckOutcome::Available(update) => update,
        UpdateCheckOutcome::UpToDate {
            current_version, ..
        } => {
            ui.ask(Page::UpToDate {
                current: current_version,
            });
            return;
        }
        UpdateCheckOutcome::Error { message } => {
            ui.ask(Page::CheckFailed { reason: message });
            return;
        }
    };

    let choice = ui.ask(Page::Available {
        current: update.current_version.clone(),
        latest: update.latest_version.clone(),
        size: update.size,
    });
    if choice != Action::Update {
        return;
    }

    // ここまでは本体に触れない。失敗・中止しても本体は動作したまま
    let package = match prepare(ui, ops, &update) {
        Ok(package) => package,
        Err(err) => {
            ops.cleanup(&update);
            let page = if err == UpdateError::Canceled {
                Page::Canceled
            } else {
                Page::Failed {
                    reason: err.to_string(),
                    log: None,
                    can_launch: false,
                }
            };
            ui.ask(page);
            return;
        }
    };

    ui.show(Page::WaitingExit);
    let stop = match ops.stop_app(EXIT_TIMEOUT) {
        Ok(stop) => stop,
        Err(err) => {
            ops.cleanup(&update);
            ui.ask(Page::Failed {
                reason: err.to_string(),
                log: None,
                can_launch: false,
            });
            return;
        }
    };
    let restart = stop == AppStop::Requested;

    ui.show(Page::Installing {
        latest: update.latest_version.clone(),
        restart,
    });
    let installed = ops.install(&package);
    ops.cleanup(&update);
    match installed {
        Ok(InstallOutcome::Installed) if restart => {
            ui.show(Page::Starting);
            if let Err(err) = ops.start() {
                finish_failed(ui, ops, &err, None);
            }
        }
        Ok(InstallOutcome::Installed) => {
            let action = ui.ask(Page::Installed {
                latest: update.latest_version,
            });
            if action == Action::LaunchApp
                && let Err(err) = ops.start()
            {
                finish_failed(ui, ops, &err, None);
            }
        }
        Ok(InstallOutcome::RebootRequired) => {
            ui.ask(Page::RebootRequired {
                latest: update.latest_version,
            });
        }
        Err(err) => {
            let log = ops.log_path();
            finish_failed(ui, ops, &err, log.as_deref());
        }
    }
}

/// 本体を動かしたまま、ダウンロード・検証・展開を済ませる
fn prepare(
    ui: &impl Ui,
    ops: &mut impl Ops,
    update: &AvailableUpdate,
) -> Result<PreparedPackage, UpdateError> {
    ops.ensure_updatable()?;
    let total = update.size;
    let downloading = |received| Page::Downloading {
        latest: update.latest_version.clone(),
        received,
        total,
    };
    ui.show(downloading(0));

    let step = (total / PROGRESS_STEPS).max(1);
    let mut shown = 0;
    ops.prepare(update, &mut |received| {
        if ui.cancel_requested() {
            return false;
        }
        if received.saturating_sub(shown) >= step || received == total {
            shown = received;
            ui.show(downloading(received));
        }
        true
    })
}

/// 本体を終了させた後の失敗を表示し、利用者が求めれば本体を起動する
///
/// 起動にも失敗した場合は、その理由を表示し直す
fn finish_failed(ui: &impl Ui, ops: &impl Ops, err: &UpdateError, log: Option<&Path>) {
    let mut reason = err.to_string();
    loop {
        let action = ui.ask(Page::Failed {
            reason,
            log: log.map(Path::to_path_buf),
            can_launch: true,
        });
        if action != Action::LaunchApp {
            return;
        }
        match ops.start() {
            Ok(()) => return,
            Err(err) => reason = err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests;
