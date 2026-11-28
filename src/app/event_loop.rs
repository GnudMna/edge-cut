//! ウィンドウ・`WebView` の生成とイベントループ
//!
//! UI からのコマンド・ドラッグ & ドロップ・ワーカーの完了通知を 1 本のイベントループで受け、
//! `App` へ反映してから画面状態を UI へ送る。ウィンドウは枠なしで、タイトルバーは UI が描く

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use tao::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopWindowTarget};
use tao::platform::run_return::EventLoopExtRunReturn;
use tao::platform::windows::WindowBuilderExtWindows;
use tao::window::{ResizeDirection, Theme, Window, WindowBuilder};
use wry::{
    DragDropEvent, NewWindowResponse, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows,
};

use super::icon;
use super::ipc::{
    BundledView, Command, Effect, ResizeEdge, WindowAction, WindowView, parse_command,
};
use super::protocol::{self, PreviewSlot};
use super::settings_store::SettingsStore;
use super::state::{App, CloseConfirm, Settings, ThemeMode, WindowGeometry};
use super::waker::Waker;
use crate::consts::{
    APP_NAME, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH,
};
use crate::install_layout;

/// ダークテーマの背景色 (UI の読み込み前に見える色。UI の `--bg-app` と揃える)
const DARK_BACKGROUND: (u8, u8, u8, u8) = (0x16, 0x17, 0x1a, 0xff);

/// ライトテーマの背景色 (UI の `--bg-app` と揃える)
const LIGHT_BACKGROUND: (u8, u8, u8, u8) = (0xf3, 0xf4, 0xf6, 0xff);

// ======================================================================
// イベント
// ======================================================================
/// ほかのスレッドからイベントループへ送るイベント
#[derive(Debug)]
enum UserEvent {
    /// ワーカーの完了通知 (結果はチャネルから取り込む)
    Wake,
    /// UI からのコマンド (JSON)
    Ipc(String),
    /// ファイルのドラッグ & ドロップ
    DragDrop(DragDropEvent),
}

// ======================================================================
// 起動
// ======================================================================
/// ウィンドウと `WebView` を作り、閉じられるまでイベントループを回す
///
/// # Returns
/// * `Result<(), Box<dyn std::error::Error>>` - 正常終了、または初期化エラー
pub(super) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let store = SettingsStore::locate();
    let (prefs, load_notice) = store
        .as_ref()
        .map_or_else(|| (Settings::default(), None), SettingsStore::load);

    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let wake_proxy = Mutex::new(proxy.clone());
    let waker = Waker::new(move || {
        if let Ok(proxy) = wake_proxy.lock() {
            let _ = proxy.send_event(UserEvent::Wake);
        }
    });

    let mut app = App::new(prefs, waker);
    if let Some(notice) = load_notice {
        app.status = notice;
    }
    let window = build_window(&event_loop, &app.prefs)?;
    app.set_dialog_parent(&window);

    let preview = PreviewSlot::default();
    let mut web_context = WebContext::new(webview_data_dir());
    let webview = build_webview(&window, &mut web_context, &preview, &proxy, &app.prefs)?;

    let mut state = LoopState {
        app,
        window,
        webview,
        preview,
        store,
        bundled: BundledView::locate(),
        drag_hover: false,
        ready: false,
        live_edit: false,
        last_sent: String::new(),
        last_saved: None,
        restore_geometry: None,
    };
    state.remember_geometry();

    event_loop.run_return(|event, _, control_flow| state.handle(event, control_flow));

    // 先に WebView とウィンドウを破棄し、画面から消えた状態で保存スレッドの完了を待つ
    let LoopState {
        app,
        window,
        webview,
        ..
    } = state;
    drop(webview);
    drop(window);
    drop(app);
    Ok(())
}

/// 枠なしのウィンドウを作る
///
/// 前回終了時の位置と大きさがいずれかのモニターに収まっていれば復元する。
/// UI の読み込みが終わるまで表示しない (白い画面がちらつかないようにする)
///
/// # Arguments
/// * `event_loop` - イベントループ
/// * `prefs` - 設定 (テーマ・前回の位置と大きさ)
///
/// # Returns
/// * `Result<Window, tao::error::OsError>` - 作ったウィンドウ
fn build_window(
    event_loop: &EventLoop<UserEvent>,
    prefs: &Settings,
) -> Result<Window, tao::error::OsError> {
    let mut builder = WindowBuilder::new()
        .with_title(APP_NAME)
        .with_inner_size(LogicalSize::new(
            DEFAULT_WINDOW_WIDTH,
            DEFAULT_WINDOW_HEIGHT,
        ))
        .with_min_inner_size(LogicalSize::new(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT))
        .with_decorations(false)
        .with_undecorated_shadow(true)
        .with_visible(false)
        .with_theme(window_theme(prefs.theme))
        .with_window_icon(icon::load_icon());
    if let Some(geometry) = prefs.window.filter(|g| fits_any_monitor(event_loop, g)) {
        builder = builder
            .with_position(PhysicalPosition::new(geometry.x, geometry.y))
            .with_inner_size(PhysicalSize::new(geometry.width, geometry.height))
            .with_maximized(geometry.maximized);
    }
    builder.build(event_loop)
}

/// ウィンドウを動かせるとみなすために、タイトルバー領域がモニター内に必要な幅 (物理 px)
const MIN_VISIBLE_TITLE_WIDTH: i64 = 120;

/// 同じく、タイトルバー領域がモニター内に必要な高さ (物理 px)
const MIN_VISIBLE_TITLE_HEIGHT: i64 = 32;

/// タイトルバーの領域がいずれかのモニターに一定以上入っているか
///
/// 前回使ったモニターが外された場合や配置が変わった場合に、つかんで動かせない位置へ
/// ウィンドウを出さないために使う
fn fits_any_monitor(event_loop: &EventLoopWindowTarget<UserEvent>, g: &WindowGeometry) -> bool {
    event_loop.available_monitors().any(|m| {
        let (pos, size) = (m.position(), m.size());
        title_bar_visible(g, (pos.x, pos.y, size.width, size.height))
    })
}

/// タイトルバーの領域 (ウィンドウ上端の帯) がモニターと十分に重なるか
///
/// # Arguments
/// * `g` - 復元するウィンドウの位置と大きさ
/// * `monitor` - モニターの左上の x・y 座標と幅・高さ (物理 px)
///
/// # Returns
/// * `bool` - 重なりが幅・高さとも必要量以上 (ウィンドウが小さい場合はその大きさ) なら `true`
fn title_bar_visible(g: &WindowGeometry, monitor: (i32, i32, u32, u32)) -> bool {
    let (mx, my, mw, mh) = monitor;
    let left = i64::from(g.x).max(i64::from(mx));
    let right = (i64::from(g.x) + i64::from(g.width)).min(i64::from(mx) + i64::from(mw));
    let top = i64::from(g.y).max(i64::from(my));
    let bottom = (i64::from(g.y) + MIN_VISIBLE_TITLE_HEIGHT).min(i64::from(my) + i64::from(mh));
    right - left >= MIN_VISIBLE_TITLE_WIDTH.min(i64::from(g.width))
        && bottom - top >= MIN_VISIBLE_TITLE_HEIGHT
}

/// テーマの指定をウィンドウのテーマにする (`System` は OS に任せる)
fn window_theme(theme: Option<ThemeMode>) -> Option<Theme> {
    match theme {
        Some(ThemeMode::Dark) => Some(Theme::Dark),
        Some(ThemeMode::Light) => Some(Theme::Light),
        Some(ThemeMode::System) | None => None,
    }
}

/// `WebView2` のユーザーデータを置くフォルダ (`%LOCALAPPDATA%\EdgeCut\WebView2`)
///
/// 既定では実行ファイルの隣に作られ、インストール先に書き込めない場合に起動できないため指定する
/// ポータブル版は `<実行フォルダ>\data\WebView2` に置く
fn webview_data_dir() -> Option<PathBuf> {
    Some(install_layout::local_data_dir()?.join("WebView2"))
}

/// UI を表示する `WebView` を作る
///
/// UI のオリジン以外への遷移と新しいウィンドウは開かせない。コマンドも UI のオリジンからのものだけ受け付ける
///
/// # Arguments
/// * `window` - 載せるウィンドウ
/// * `web_context` - `WebView2` のユーザーデータの場所
/// * `preview` - カスタムプロトコルが返すプレビュー画像
/// * `proxy` - イベントループへイベントを送る
/// * `prefs` - 設定 (読み込み前の背景色に使う)
///
/// # Returns
/// * `wry::Result<WebView>` - 作った `WebView`
fn build_webview(
    window: &Window,
    web_context: &mut WebContext,
    preview: &PreviewSlot,
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
    prefs: &Settings,
) -> wry::Result<WebView> {
    let dark = match prefs.theme {
        Some(ThemeMode::Dark) => true,
        Some(ThemeMode::Light) => false,
        Some(ThemeMode::System) | None => window.theme() == Theme::Dark,
    };
    let preview = preview.clone();
    let ipc_proxy = proxy.clone();
    let drop_proxy = proxy.clone();
    WebViewBuilder::new_with_web_context(web_context)
        .with_custom_protocol(protocol::SCHEME.into(), move |_id, request| {
            protocol::respond(&preview, &request)
        })
        .with_url(protocol::START_URL)
        .with_background_color(if dark {
            DARK_BACKGROUND
        } else {
            LIGHT_BACKGROUND
        })
        .with_ipc_handler(move |request| {
            if request.uri().to_string().starts_with(protocol::ORIGIN) {
                let _ = ipc_proxy.send_event(UserEvent::Ipc(request.into_body()));
            }
        })
        .with_drag_drop_handler(move |event| {
            let _ = drop_proxy.send_event(UserEvent::DragDrop(event));
            true
        })
        .with_navigation_handler(|url| url.starts_with(protocol::ORIGIN))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_download_started_handler(|_, _| false)
        .with_devtools(cfg!(debug_assertions))
        .with_hotkeys_zoom(false)
        .with_browser_accelerator_keys(false)
        .with_default_context_menus(cfg!(debug_assertions))
        .build(window)
}

// ======================================================================
// イベントループ
// ======================================================================
/// イベントループが持つ状態
struct LoopState {
    app: App,
    window: Window,
    webview: WebView,
    /// カスタムプロトコルへ渡すプレビュー画像
    preview: PreviewSlot,
    /// 設定ファイル (保存先を決められない場合は `None`)
    store: Option<SettingsStore>,
    /// 同梱の文書の有無
    bundled: BundledView,
    /// ファイルをドラッグして重ねているか
    drag_hover: bool,
    /// UI の初期化が終わったか (終わるまで画面状態を送らない)
    ready: bool,
    /// スライダーのドラッグ中など、設定を続けて変えている途中か (終わるまで保存しない)
    live_edit: bool,
    /// 直近に UI へ送った画面状態 (JSON)
    last_sent: String,
    /// 直近に保存した設定 (JSON)
    last_saved: Option<String>,
    /// 最大化していないときの位置と大きさ (最大化したまま終了しても元の大きさを保存する)
    restore_geometry: Option<WindowGeometry>,
}

impl LoopState {
    /// イベントを 1 つ処理する
    ///
    /// # Arguments
    /// * `event` - イベント
    /// * `control_flow` - 次のイベントまでの待ち方
    fn handle(&mut self, event: Event<'_, UserEvent>, control_flow: &mut ControlFlow) {
        match event {
            Event::NewEvents(StartCause::Init) => *control_flow = ControlFlow::Wait,
            Event::UserEvent(UserEvent::Ipc(json)) => self.handle_ipc(&json, control_flow),
            Event::UserEvent(UserEvent::DragDrop(event)) => self.handle_drag_drop(event),
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => self.request_close(control_flow),
                WindowEvent::Resized(_) | WindowEvent::Moved(_) => self.remember_geometry(),
                _ => {}
            },
            Event::MainEventsCleared => self.tick(control_flow),
            _ => {}
        }
    }

    /// UI からのコマンドを処理する
    fn handle_ipc(&mut self, json: &str, control_flow: &mut ControlFlow) {
        let command = match parse_command(json) {
            Ok(command) => command,
            Err(message) => {
                eprintln!("{message}");
                return;
            }
        };
        self.live_edit = matches!(command, Command::SetTrim { live: true, .. });
        match self.app.handle_command(command) {
            Effect::None => {}
            Effect::Ready => {
                self.ready = true;
                // 再読み込み後も最新の状態を送り直す
                self.last_sent.clear();
                self.window.set_visible(true);
                self.window.set_focus();
                let _ = self.webview.focus();
            }
            Effect::Window(action) => self.handle_window_action(action, control_flow),
        }
        // テーマはウィンドウ (タイトルバーの無い影の色など) にも反映する
        self.window.set_theme(window_theme(self.app.prefs.theme));
    }

    /// ウィンドウを操作する
    fn handle_window_action(&mut self, action: WindowAction, control_flow: &mut ControlFlow) {
        match action {
            WindowAction::Minimize => self.window.set_minimized(true),
            WindowAction::ToggleMaximize => {
                self.window.set_maximized(!self.window.is_maximized());
            }
            WindowAction::Close => self.request_close(control_flow),
            WindowAction::Drag => {
                let _ = self.window.drag_window();
            }
            WindowAction::Resize { edge } => {
                let direction = match edge {
                    ResizeEdge::North => ResizeDirection::North,
                    ResizeEdge::NorthEast => ResizeDirection::NorthEast,
                    ResizeEdge::NorthWest => ResizeDirection::NorthWest,
                };
                let _ = self.window.drag_resize_window(direction);
            }
        }
    }

    /// ドラッグ & ドロップを処理する
    fn handle_drag_drop(&mut self, event: DragDropEvent) {
        match event {
            DragDropEvent::Enter { .. } => self.drag_hover = true,
            DragDropEvent::Leave => self.drag_hover = false,
            DragDropEvent::Drop { paths, .. } => {
                self.drag_hover = false;
                self.app.handle_dropped_paths(paths);
            }
            _ => {}
        }
    }

    /// ウィンドウを閉じる要求を処理する
    ///
    /// 一括トリミングの実行中は確認ダイアログを出して閉じない。閉じるときは先にウィンドウを隠し、
    /// 設定を保存してからイベントループを終える (保存スレッドの完了は `App` の破棄で待つ)
    fn request_close(&mut self, control_flow: &mut ControlFlow) {
        if self.app.batch.running && self.app.batch.close_confirm != CloseConfirm::Confirmed {
            self.app.batch.close_confirm = CloseConfirm::Asking;
            return;
        }
        self.remember_geometry();
        self.app.prefs.window = self.restore_geometry.map(|g| WindowGeometry {
            maximized: self.window.is_maximized(),
            ..g
        });
        self.window.set_visible(false);
        self.save_settings(true);
        *control_flow = ControlFlow::Exit;
    }

    /// 最大化していないときの位置と大きさを記録する
    fn remember_geometry(&mut self) {
        if self.window.is_maximized() || self.window.is_minimized() {
            return;
        }
        let Ok(pos) = self.window.outer_position() else {
            return;
        };
        let size = self.window.inner_size();
        self.restore_geometry = Some(WindowGeometry {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
            maximized: false,
        });
    }

    /// ワーカーの結果を取り込み、画面状態を UI へ送る
    ///
    /// イベントをまとめて処理した後に 1 回だけ呼ぶ
    fn tick(&mut self, control_flow: &mut ControlFlow) {
        if *control_flow == ControlFlow::Exit {
            return;
        }
        let app = &mut self.app;
        app.poll_batch_result();
        app.poll_io();
        app.poll_trim_work();
        app.poll_update();
        if app.close_requested {
            app.close_requested = false;
            self.request_close(control_flow);
            if *control_flow == ControlFlow::Exit {
                return;
            }
        }

        self.sync_preview();
        self.send_snapshot();
        if !self.live_edit {
            self.save_settings(false);
        }

        // デバウンス中の再計算は期限で起きて開始する
        *control_flow = match self.app.trim_worker.debounce_until {
            Some(until) if until > Instant::now() => ControlFlow::WaitUntil(until),
            Some(_) => ControlFlow::Poll,
            None => ControlFlow::Wait,
        };
    }

    /// カスタムプロトコルが返すプレビュー画像を最新にする
    fn sync_preview(&self) {
        let image = &self.app.image;
        let id = image.preview.as_ref().map(|_| image.preview_id);
        if self.preview.id() != id {
            self.preview.set(image.preview_id, image.preview.clone());
        }
    }

    /// 画面状態が変わっていれば UI へ送る
    fn send_snapshot(&mut self) {
        if !self.ready {
            return;
        }
        let window = WindowView {
            maximized: self.window.is_maximized(),
            drag_hover: self.drag_hover,
        };
        let snapshot = self.app.snapshot(self.bundled, window);
        let Ok(json) = serde_json::to_string(&snapshot) else {
            return;
        };
        if json == self.last_sent {
            return;
        }
        let script = format!("window.__edgecut&&window.__edgecut.receive({json})");
        if self.webview.evaluate_script(&script).is_ok() {
            self.last_sent = json;
        }
    }

    /// 設定が変わっていればファイルへ保存する
    ///
    /// # Arguments
    /// * `force` - 変わっていなくても保存する (終了時)
    fn save_settings(&mut self, force: bool) {
        let Some(store) = &self.store else {
            return;
        };
        let settings = self.app.settings_to_save();
        let Ok(json) = serde_json::to_string(&settings) else {
            return;
        };
        if !force && self.last_saved.as_deref() == Some(json.as_str()) {
            return;
        }
        // 初回は読み込んだ内容と同じなら書かない (起動しただけで設定ファイルを作らない)
        if self.last_saved.is_none() && !force {
            self.last_saved = Some(json);
            return;
        }
        if let Err(e) = store.save(&settings) {
            eprintln!("設定の保存に失敗: {e}");
        }
        self.last_saved = Some(json);
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: (i32, i32, u32, u32) = (0, 0, 1920, 1080);

    fn geometry(x: i32, y: i32) -> WindowGeometry {
        WindowGeometry {
            x,
            y,
            width: 1200,
            height: 720,
            maximized: false,
        }
    }

    /// モニター内に収まる位置は復元すること
    #[test]
    fn window_inside_monitor_is_restored() {
        assert!(title_bar_visible(&geometry(100, 100), MONITOR));
    }

    /// 左上が画面内でも、大半が右へはみ出してつかめない位置は復元しないこと
    #[test]
    fn window_mostly_off_right_edge_is_rejected() {
        assert!(!title_bar_visible(&geometry(1900, 100), MONITOR));
    }

    /// タイトルバーが画面の上へはみ出す位置は復元しないこと
    #[test]
    fn window_above_monitor_is_rejected() {
        assert!(!title_bar_visible(&geometry(100, -40), MONITOR));
    }

    /// 左へ大きくはみ出しても、タイトルバーが十分見えていれば復元すること
    #[test]
    fn window_partly_off_left_edge_is_restored() {
        assert!(title_bar_visible(&geometry(-1000, 100), MONITOR));
    }

    /// 下端の外にあるウィンドウは復元しないこと
    #[test]
    fn window_below_monitor_is_rejected() {
        assert!(!title_bar_visible(&geometry(100, 1070), MONITOR));
    }
}
