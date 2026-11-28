//! 更新プログラム (`EdgeCutUpdater.exe`) の画面
//!
//! tao の小さなウィンドウに wry の `WebView` を載せ、本体と同じ UI の更新画面 (`?view=updater`) を表示する。
//! 更新の進行 (`update::flow`) は別スレッドで動かし、`bridge` を通してページと利用者の操作をやり取りする

mod bridge;
mod view;

use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;

use tao::dpi::LogicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::run_return::EventLoopExtRunReturn;
use tao::window::{Window, WindowBuilder};
use wry::{NewWindowResponse, WebContext, WebView, WebViewBuilder};

use super::icon;
use super::protocol::{self, PreviewSlot};
use crate::install_layout::is_portable_dir;
use crate::panic_message::catch_panic;
use crate::update::flow::{Page, Ui, run_flow};
use crate::update::{SystemOps, UpdateError, UpdaterArgs};
use bridge::Bridge;
use view::{PageView, UpdaterCommand, UpdaterSnapshot};

// ======================================================================
// 定数
// ======================================================================
/// ウィンドウのタイトル
const WINDOW_TITLE: &str = "EdgeCut の更新";
/// ウィンドウの大きさ (論理ピクセル)
const WINDOW_SIZE: (f64, f64) = (480.0, 340.0);

// ======================================================================
// イベント
// ======================================================================
/// ほかのスレッドからイベントループへ送るイベント
#[derive(Debug)]
enum UserEvent {
    /// ページが変わった
    Wake,
    /// UI からのコマンド (JSON)
    Ipc(String),
    /// 更新の進行が終わった
    Finished,
}

// ======================================================================
// エントリポイント
// ======================================================================
/// 更新プログラムを実行する
///
/// 本体から渡された引数を確かめ、更新画面を閉じるまで表示する
///
/// # Returns
/// * `ExitCode` - 画面を表示できた場合は成功 (更新の成否は画面で知らせる)
pub fn run_updater() -> ExitCode {
    crate::panic_hook::install();
    let Some(args) = UpdaterArgs::parse(std::env::args_os().skip(1)) else {
        show_error("EdgeCut のメニューの「更新を確認…」から更新してください");
        return ExitCode::FAILURE;
    };
    match run_window(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            show_error(&format!(
                "更新画面を表示できないため、更新を中止しました。\n\n詳細: {err}"
            ));
            ExitCode::FAILURE
        }
    }
}

/// エラーをダイアログで知らせる
fn show_error(message: &str) {
    rfd::MessageDialog::new()
        .set_title(WINDOW_TITLE)
        .set_level(rfd::MessageLevel::Error)
        .set_description(message)
        .show();
}

// ======================================================================
// ウィンドウ
// ======================================================================
/// ウィンドウと `WebView` を作り、更新の進行が終わるか閉じられるまでイベントループを回す
fn run_window(args: &UpdaterArgs) -> Result<(), Box<dyn Error>> {
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let wake_proxy = std::sync::Mutex::new(proxy.clone());
    let bridge = Bridge::new(Page::Checking, move || {
        if let Ok(proxy) = wake_proxy.lock() {
            let _ = proxy.send_event(UserEvent::Wake);
        }
    });

    let window = WindowBuilder::new()
        .with_title(WINDOW_TITLE)
        .with_inner_size(LogicalSize::new(WINDOW_SIZE.0, WINDOW_SIZE.1))
        .with_resizable(false)
        .with_maximizable(false)
        .with_visible(false)
        .with_window_icon(icon::load_icon())
        .build(&event_loop)?;
    let mut web_context = WebContext::new(webview_data_dir());
    let webview = build_webview(&window, &mut web_context, &proxy)?;

    let mut state = LoopState {
        window,
        webview,
        bridge,
        portable: is_portable_dir(&args.install_dir),
        flow: Some((args.clone(), proxy)),
        last_sent: String::new(),
    };
    event_loop.run_return(|event, _, control_flow| state.handle(event, control_flow));
    Ok(())
}

/// 更新プログラムの `WebView2` のユーザーデータを置くフォルダ (自身のコピー先の `WebView2`)
///
/// 本体のユーザーデータとは分け、更新の後片付けでまとめて消す
fn webview_data_dir() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("WebView2"))
}

/// 更新画面を表示する `WebView` を作る
fn build_webview(
    window: &Window,
    web_context: &mut WebContext,
    proxy: &EventLoopProxy<UserEvent>,
) -> wry::Result<WebView> {
    let preview = PreviewSlot::default();
    let ipc_proxy = proxy.clone();
    WebViewBuilder::new_with_web_context(web_context)
        .with_custom_protocol(protocol::SCHEME.into(), move |_id, request| {
            protocol::respond(&preview, &request)
        })
        .with_url(protocol::UPDATER_URL)
        .with_ipc_handler(move |request| {
            if request.uri().to_string().starts_with(protocol::ORIGIN) {
                let _ = ipc_proxy.send_event(UserEvent::Ipc(request.into_body()));
            }
        })
        .with_navigation_handler(|url| url.starts_with(protocol::ORIGIN))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_download_started_handler(|_, _| false)
        .with_devtools(cfg!(debug_assertions))
        .with_hotkeys_zoom(false)
        .build(window)
}

// ======================================================================
// イベントループ
// ======================================================================
/// イベントループが持つ状態
struct LoopState {
    window: Window,
    webview: WebView,
    bridge: Bridge,
    /// ポータブル版を更新するか
    portable: bool,
    /// まだ始めていない更新の進行 (UI の初期化後に始める)
    flow: Option<(UpdaterArgs, EventLoopProxy<UserEvent>)>,
    /// 直近に UI へ送った画面状態 (JSON)
    last_sent: String,
}

impl LoopState {
    /// イベントを 1 つ処理する
    fn handle(&mut self, event: Event<'_, UserEvent>, control_flow: &mut ControlFlow) {
        match event {
            Event::NewEvents(StartCause::Init) => *control_flow = ControlFlow::Wait,
            Event::UserEvent(UserEvent::Wake) => self.send_snapshot(),
            Event::UserEvent(UserEvent::Ipc(json)) => self.handle_ipc(&json),
            Event::UserEvent(UserEvent::Finished) => *control_flow = ControlFlow::Exit,
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } if self.bridge.request_close() => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    }

    /// UI からのコマンドを処理する
    fn handle_ipc(&mut self, json: &str) {
        let Ok(command) = serde_json::from_str::<UpdaterCommand>(json) else {
            eprintln!("更新画面のコマンドを解釈できない: {json}");
            return;
        };
        match command {
            UpdaterCommand::Ready => {
                self.last_sent.clear();
                self.send_snapshot();
                self.window.set_visible(true);
                self.window.set_focus();
                self.start_flow();
            }
            UpdaterCommand::Choose { action } => self.bridge.choose(action.into()),
            UpdaterCommand::Cancel => self.bridge.cancel(),
        }
    }

    /// 更新の進行を別スレッドで始める (UI の再読み込みでは始め直さない)
    ///
    /// 進行が panic した場合は内部エラーとして結果を表示する
    fn start_flow(&mut self) {
        let Some((args, proxy)) = self.flow.take() else {
            return;
        };
        let ui = self.bridge.clone();
        thread::spawn(move || {
            let mut ops = SystemOps::new(&args);
            if let Err(message) = catch_panic(|| run_flow(&ui, &mut ops)) {
                eprintln!("更新処理が異常終了した: {message}");
                ui.ask(Page::Failed {
                    reason: format!("{} ({message})", UpdateError::Internal),
                    log: None,
                    can_launch: false,
                });
            }
            let _ = proxy.send_event(UserEvent::Finished);
        });
    }

    /// 画面状態が変わっていれば UI へ送る
    fn send_snapshot(&mut self) {
        let snapshot = UpdaterSnapshot {
            version: crate::update::current_app_version(),
            portable: self.portable,
            page: PageView::from(&self.bridge.page()),
        };
        let Ok(json) = serde_json::to_string(&snapshot) else {
            return;
        };
        if json == self.last_sent {
            return;
        }
        let script = format!("window.__edgecutUpdater&&window.__edgecutUpdater.receive({json})");
        if self.webview.evaluate_script(&script).is_ok() {
            self.last_sent = json;
        }
    }
}
