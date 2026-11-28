//! 更新プログラムの進行のテスト
//!
//! 画面と副作用を模して、各分岐で表示するページと行う副作用の順序を確かめる

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use super::{Action, AppStop, Ops, Page, Ui, run_flow};
use crate::update::error::UpdateError;
use crate::update::types::{AvailableUpdate, InstallOutcome, PreparedPackage, UpdateCheckOutcome};

// ======================================================================
// 模擬
// ======================================================================
/// 表示したページを記録し、用意した操作を順に返す画面
#[derive(Default)]
struct FakeUi {
    pages: RefCell<Vec<Page>>,
    actions: RefCell<VecDeque<Action>>,
    cancel: bool,
}

impl FakeUi {
    /// 操作の順序を指定して作る
    fn with_actions(actions: &[Action]) -> Self {
        Self {
            actions: RefCell::new(actions.iter().copied().collect()),
            ..Self::default()
        }
    }

    /// 表示したページ (進捗の途中経過を除く)
    fn pages(&self) -> Vec<Page> {
        let mut pages = self.pages.borrow().clone();
        pages.dedup_by(|a, b| {
            matches!((a, b), (Page::Downloading { .. }, Page::Downloading { .. }))
        });
        pages
    }

    /// 最後に表示したページ
    fn last(&self) -> Page {
        self.pages.borrow().last().cloned().expect("page")
    }
}

impl Ui for FakeUi {
    fn show(&self, page: Page) {
        self.pages.borrow_mut().push(page);
    }

    fn ask(&self, page: Page) -> Action {
        self.pages.borrow_mut().push(page);
        self.actions
            .borrow_mut()
            .pop_front()
            .unwrap_or(Action::Close)
    }

    fn cancel_requested(&self) -> bool {
        self.cancel
    }
}

/// 結果を差し替えられる副作用。呼ばれた順に名前を記録する
struct FakeOps {
    check: UpdateCheckOutcome,
    updatable: Result<(), UpdateError>,
    prepare: Result<PreparedPackage, UpdateError>,
    stop: Result<AppStop, UpdateError>,
    install: Result<InstallOutcome, UpdateError>,
    start: RefCell<VecDeque<Result<(), UpdateError>>>,
    calls: RefCell<Vec<&'static str>>,
}

impl Default for FakeOps {
    fn default() -> Self {
        Self {
            check: UpdateCheckOutcome::Available(sample_update()),
            updatable: Ok(()),
            prepare: Ok(PreparedPackage::Msi {
                path: PathBuf::from("a.msi"),
            }),
            stop: Ok(AppStop::Requested),
            install: Ok(InstallOutcome::Installed),
            start: RefCell::new(VecDeque::new()),
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl FakeOps {
    fn record(&self, name: &'static str) {
        self.calls.borrow_mut().push(name);
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.borrow().clone()
    }
}

impl Ops for FakeOps {
    fn check(&self) -> UpdateCheckOutcome {
        self.record("check");
        self.check.clone()
    }

    fn ensure_updatable(&mut self) -> Result<(), UpdateError> {
        self.record("ensure");
        self.updatable.clone()
    }

    fn prepare(
        &self,
        update: &AvailableUpdate,
        on_progress: &mut dyn FnMut(u64) -> bool,
    ) -> Result<PreparedPackage, UpdateError> {
        self.record("prepare");
        for received in [update.size / 2, update.size] {
            if !on_progress(received) {
                return Err(UpdateError::Canceled);
            }
        }
        self.prepare.clone()
    }

    fn stop_app(&self, _timeout: Duration) -> Result<AppStop, UpdateError> {
        self.record("stop");
        self.stop.clone()
    }

    fn install(&mut self, _package: &PreparedPackage) -> Result<InstallOutcome, UpdateError> {
        self.record("install");
        self.install.clone()
    }

    fn start(&self) -> Result<(), UpdateError> {
        self.record("start");
        self.start.borrow_mut().pop_front().unwrap_or(Ok(()))
    }

    fn log_path(&self) -> Option<PathBuf> {
        Some(PathBuf::from("updater-msi.log"))
    }

    fn cleanup(&self, _update: &AvailableUpdate) {
        self.record("cleanup");
    }
}

/// 確認で見つかる更新を模す
fn sample_update() -> AvailableUpdate {
    AvailableUpdate {
        current_version: "1.0.0".into(),
        latest_version: "1.1.0".into(),
        tag: "v1.1.0".into(),
        asset_name: "edge-cut-1.1.0-x86_64.msi".into(),
        size: 1000,
        sha256: "0".repeat(64),
    }
}

/// 更新ありのページ
fn available_page() -> Page {
    Page::Available {
        current: "1.0.0".into(),
        latest: "1.1.0".into(),
        size: 1000,
    }
}

/// 進捗を含めたダウンロード中のページ
fn downloading(received: u64) -> Page {
    Page::Downloading {
        latest: "1.1.0".into(),
        received,
        total: 1000,
    }
}

// ======================================================================
// テスト: 正常系
// ======================================================================
/// 更新を選ぶと、ダウンロード・本体の終了・インストール・起動の順に進むこと
#[test]
fn update_runs_to_restart() {
    let ui = FakeUi::with_actions(&[Action::Update]);
    let mut ops = FakeOps::default();

    run_flow(&ui, &mut ops);

    assert_eq!(
        ops.calls(),
        [
            "check", "ensure", "prepare", "stop", "install", "cleanup", "start"
        ]
    );
    assert_eq!(
        ui.pages(),
        [
            Page::Checking,
            available_page(),
            downloading(0),
            Page::WaitingExit,
            Page::Installing {
                latest: "1.1.0".into(),
                restart: true,
            },
            Page::Starting,
        ]
    );
    // 進捗は最後に全体の大きさまで届く
    assert!(ui.pages.borrow().contains(&downloading(1000)));
}

/// 利用者が本体を閉じていた場合は、インストールまで進めて起動しないこと
#[test]
fn app_closed_by_user_installs_without_restart() {
    let ui = FakeUi::with_actions(&[Action::Update, Action::Close]);
    let mut ops = FakeOps {
        stop: Ok(AppStop::ClosedByUser),
        ..FakeOps::default()
    };

    run_flow(&ui, &mut ops);

    assert_eq!(
        ops.calls(),
        ["check", "ensure", "prepare", "stop", "install", "cleanup"]
    );
    // インストール中から起動しないことを示し、完了を知らせる
    assert!(ui.pages().contains(&Page::Installing {
        latest: "1.1.0".into(),
        restart: false,
    }));
    assert_eq!(
        ui.last(),
        Page::Installed {
            latest: "1.1.0".into()
        }
    );
}

/// 本体を閉じていた場合も、求めに応じて新しい版を起動すること
#[test]
fn app_closed_by_user_can_launch_after_install() {
    let ui = FakeUi::with_actions(&[Action::Update, Action::LaunchApp]);
    let mut ops = FakeOps {
        stop: Ok(AppStop::ClosedByUser),
        ..FakeOps::default()
    };

    run_flow(&ui, &mut ops);

    assert_eq!(ops.calls().last(), Some(&"start"));
}

/// 最新版なら更新せずに知らせること
#[test]
fn up_to_date_stops_after_check() {
    let ui = FakeUi::default();
    let mut ops = FakeOps {
        check: UpdateCheckOutcome::UpToDate {
            current_version: "1.1.0".into(),
            latest_version: "1.1.0".into(),
        },
        ..FakeOps::default()
    };

    run_flow(&ui, &mut ops);

    assert_eq!(ops.calls(), ["check"]);
    assert_eq!(
        ui.last(),
        Page::UpToDate {
            current: "1.1.0".into()
        }
    );
}

/// 更新ありで閉じた場合は何もしないこと
#[test]
fn closing_available_page_does_nothing() {
    let ui = FakeUi::with_actions(&[Action::Close]);
    let mut ops = FakeOps::default();

    run_flow(&ui, &mut ops);

    assert_eq!(ops.calls(), ["check"]);
    assert_eq!(ui.last(), available_page());
}

/// 再起動が必要な場合は本体を起動せずに知らせること
#[test]
fn reboot_required_is_reported() {
    let ui = FakeUi::with_actions(&[Action::Update]);
    let mut ops = FakeOps {
        install: Ok(InstallOutcome::RebootRequired),
        ..FakeOps::default()
    };

    run_flow(&ui, &mut ops);

    assert!(!ops.calls().contains(&"start"));
    assert_eq!(
        ui.last(),
        Page::RebootRequired {
            latest: "1.1.0".into()
        }
    );
}

// ======================================================================
// テスト: 本体を終了させる前の失敗・中止
// ======================================================================
/// 確認に失敗したら理由を表示すること
#[test]
fn check_failure_is_reported() {
    let ui = FakeUi::default();
    let mut ops = FakeOps {
        check: UpdateCheckOutcome::Error {
            message: "通信失敗".into(),
        },
        ..FakeOps::default()
    };

    run_flow(&ui, &mut ops);

    assert_eq!(
        ui.last(),
        Page::CheckFailed {
            reason: "通信失敗".into()
        }
    );
}

/// 更新できない導入先では、ダウンロードも本体の終了もしないこと
#[test]
fn not_updatable_stops_before_download() {
    let ui = FakeUi::with_actions(&[Action::Update]);
    let mut ops = FakeOps {
        updatable: Err(UpdateError::InstallLocationMismatch),
        ..FakeOps::default()
    };

    run_flow(&ui, &mut ops);

    assert_eq!(ops.calls(), ["check", "ensure", "cleanup"]);
    assert_eq!(
        ui.last(),
        Page::Failed {
            reason: UpdateError::InstallLocationMismatch.to_string(),
            log: None,
            can_launch: false,
        }
    );
}

/// ダウンロード中に中止すると、本体を終了させずに中止を知らせること
#[test]
fn cancel_during_download_keeps_app_running() {
    let ui = FakeUi {
        cancel: true,
        ..FakeUi::with_actions(&[Action::Update])
    };
    let mut ops = FakeOps::default();

    run_flow(&ui, &mut ops);

    assert_eq!(ops.calls(), ["check", "ensure", "prepare", "cleanup"]);
    assert_eq!(ui.last(), Page::Canceled);
}

/// 検証に失敗したら本体を終了させずに理由を表示すること
#[test]
fn prepare_failure_keeps_app_running() {
    let ui = FakeUi::with_actions(&[Action::Update]);
    let mut ops = FakeOps {
        prepare: Err(UpdateError::SignatureInvalid),
        ..FakeOps::default()
    };

    run_flow(&ui, &mut ops);

    assert!(!ops.calls().contains(&"stop"));
    assert_eq!(
        ui.last(),
        Page::Failed {
            reason: UpdateError::SignatureInvalid.to_string(),
            log: None,
            can_launch: false,
        }
    );
}

/// 本体が終了しなければインストールしないこと
#[test]
fn exit_timeout_skips_install() {
    let ui = FakeUi::with_actions(&[Action::Update]);
    let mut ops = FakeOps {
        stop: Err(UpdateError::ExitTimeout),
        ..FakeOps::default()
    };

    run_flow(&ui, &mut ops);

    assert!(!ops.calls().contains(&"install"));
    assert!(matches!(
        ui.last(),
        Page::Failed {
            can_launch: false,
            ..
        }
    ));
}

// ======================================================================
// テスト: 本体を終了させた後の失敗
// ======================================================================
/// インストールに失敗したらログを案内し、求めに応じて本体を起動し直すこと
#[test]
fn install_failure_offers_launch() {
    let ui = FakeUi::with_actions(&[Action::Update, Action::LaunchApp]);
    let mut ops = FakeOps {
        install: Err(UpdateError::MsiFailed { code: 1603 }),
        ..FakeOps::default()
    };

    run_flow(&ui, &mut ops);

    assert_eq!(ops.calls().last(), Some(&"start"));
    assert_eq!(
        ui.last(),
        Page::Failed {
            reason: UpdateError::MsiFailed { code: 1603 }.to_string(),
            log: Some(PathBuf::from("updater-msi.log")),
            can_launch: true,
        }
    );
}

/// 起動し直せなかった場合は、その理由を表示し直すこと
#[test]
fn launch_failure_is_reported_again() {
    let ui = FakeUi::with_actions(&[Action::Update, Action::LaunchApp, Action::Close]);
    let restart = UpdateError::RestartFailed { detail: "x".into() };
    let mut ops = FakeOps {
        start: RefCell::new(VecDeque::from([Err(restart.clone()), Err(restart.clone())])),
        ..FakeOps::default()
    };

    run_flow(&ui, &mut ops);

    // 新しい版の起動に失敗 → 起動し直しにも失敗 → 閉じる
    assert_eq!(
        ops.calls().iter().filter(|call| **call == "start").count(),
        2
    );
    assert_eq!(
        ui.last(),
        Page::Failed {
            reason: restart.to_string(),
            log: None,
            can_launch: true,
        }
    );
}

/// ダウンロード中のページだけ中止を受け付けること
#[test]
fn only_downloading_page_is_cancelable() {
    assert!(downloading(0).is_cancelable());
    assert!(!available_page().is_cancelable());
    assert!(!Page::WaitingExit.is_cancelable());
}
