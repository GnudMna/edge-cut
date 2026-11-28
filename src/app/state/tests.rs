use super::*;
use crate::app::settings_store::{parse_legacy_value, parse_settings};
use crate::consts::{JPEG_QUALITY_DEFAULT, PADDING_MAX, TOL_MAX, WEBP_QUALITY_DEFAULT};

/// `App::default()` が画像なしで初期化されること
#[test]
fn new_has_no_images() {
    let app = App::default();
    assert!(app.image.original.is_none());
    assert!(app.image.preview.is_none());
    assert!(app.image.info.is_none());
    assert!(app.image.trim_target().is_none());
}

/// `App::default()` が許容度と余白をゼロで初期化すること
#[test]
fn new_default_tolerance_and_padding_are_zero() {
    let app = App::default();
    assert_eq!(app.prefs.tolerance, 0);
    assert_eq!(app.prefs.padding, 0);
}

/// 旧バージョンが保存した `compact_mode` があっても、残りの設定を読み込めること
///
/// コンパクト表示は表示モードの「結果」に置き換えたため、値は引き継がない
#[test]
fn app_new_ignores_legacy_compact_mode() {
    let app = app_from_legacy("(tolerance: 7, compact_mode: true)");
    assert_eq!(app.prefs.tolerance, 7);
    assert_eq!(app.prefs.preview_mode, PreviewMode::Range);
}

/// `App::default()` がステータスメッセージを適切に初期化すること
#[test]
fn new_dark_mode_enabled_by_default() {
    let app = App::default();
    assert!(app.prefs.dark_mode);
}

/// `App::default()` が自動保存モードをデフォルトで無効にすること
#[test]
fn new_auto_save_disabled_by_default() {
    let app = App::default();
    assert!(!app.prefs.auto_save);
}

/// `App::default()` がプレビューを範囲表示から始めること
#[test]
fn new_preview_mode_is_range_by_default() {
    let app = App::default();
    assert_eq!(app.prefs.preview_mode, PreviewMode::Range);
}

/// `App::default()` がステータスメッセージを適切に初期化すること
#[test]
fn new_status_message_is_not_empty() {
    let app = App::default();
    assert_ne!(app.status, "");
}

/// `App::default()` が自動保存ディレクトリを適切に初期化すること
#[test]
fn new_auto_save_dir_is_non_empty_path() {
    let app = App::default();
    assert!(!app.prefs.auto_save_dir.as_os_str().is_empty());
}
/// `settings_to_save` が範囲外の値を保存前に正規化すること
#[test]
fn save_settings_clamps_out_of_range_values() {
    let app = App {
        prefs: Settings {
            tolerance: 500,
            padding: 500,
            ..Settings::default()
        },
        ..App::default()
    };
    let s = save_and_load(&app);
    assert_eq!(s.tolerance, TOL_MAX);
    assert_eq!(s.padding, PADDING_MAX);
}
/// `settings_to_save` が手動背景色を保存しないこと
#[test]
fn save_settings_excludes_bg_override() {
    let app = App {
        prefs: Settings {
            bg_override: Some([10, 20, 30, 255]),
            ..Settings::default()
        },
        ..App::default()
    };
    let s = save_and_load(&app);
    assert_eq!(s.bg_override, None);
}

/// 保存データに手動背景色があっても、起動時は自動検出から始まること
///
/// 旧バージョンが保存した `bg_override` を読み込んでも無視する
#[test]
fn app_new_ignores_saved_bg_override() {
    let app = app_from_legacy("(tolerance: 5, bg_override: Some((40, 50, 60, 200)))");
    assert_eq!(app.prefs.tolerance, 5);
    assert_eq!(app.prefs.bg_override, None);
    assert_eq!(app.background.bg_color, None);
}

/// 保存データに一部のフィールドしかなくても、残りを既定値で補って読み込めること
#[test]
fn app_new_fills_missing_settings_fields_with_defaults() {
    let app = app_from_legacy("(tolerance: 42, preview_mode: Result)");
    let defaults = Settings::default();
    // 保存データにあるフィールドは復元される
    assert_eq!(app.prefs.tolerance, 42);
    assert_eq!(app.prefs.preview_mode, PreviewMode::Result);
    // 無いフィールドは既定値になる
    assert_eq!(app.prefs.padding, defaults.padding);
    assert_eq!(app.prefs.dark_mode, defaults.dark_mode);
    assert_eq!(app.prefs.jpeg_quality, JPEG_QUALITY_DEFAULT);
    assert_eq!(app.prefs.webp_quality, WEBP_QUALITY_DEFAULT);
}

/// 保存データに未知のフィールドがあっても、他の設定を読み込めること
#[test]
fn app_new_ignores_unknown_settings_fields() {
    let app = app_from_legacy("(tolerance: 7, removed_field: 1)");
    assert_eq!(app.prefs.tolerance, 7);
}
// ======================================================================
// ヘルパー
// ======================================================================
/// egui 版が保存した設定の値 (RON) から `App` を作る
fn app_from_legacy(value: &str) -> App {
    let prefs = parse_legacy_value(value).expect("旧形式の設定を読める");
    App::new(prefs, Waker::noop())
}

/// `settings_to_save` で保存した設定をそのまま読み戻す
fn save_and_load(app: &App) -> Settings {
    let json = serde_json::to_string(&app.settings_to_save()).expect("設定を書き出せる");
    parse_settings(&json).expect("保存済みの設定")
}

/// 完了しないスレッドは上限で待つのをやめ、完了済みのスレッドは待てること
#[test]
fn join_with_timeout_gives_up_on_stuck_thread() {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let stuck = std::thread::spawn(move || {
        let _ = rx.recv();
    });
    assert!(!join_with_timeout(
        stuck,
        Instant::now() + Duration::from_millis(50)
    ));
    // 解放してスレッドを終わらせる
    drop(tx);

    let done = std::thread::spawn(|| {});
    assert!(join_with_timeout(
        done,
        Instant::now() + Duration::from_secs(5)
    ));
}

/// 詰まった保存スレッドが複数あっても、共通の期限内に終了処理が戻ること
#[test]
fn shutdown_shares_one_deadline_across_stuck_threads() {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
    // `App::new` は一括トリミングと保存で期限を共有する
    let mut app = App::default();
    for _ in 0..3 {
        let rx = std::sync::Arc::clone(&rx);
        app.io.save_handles.push(std::thread::spawn(move || {
            let _ = rx.lock().map(|rx| rx.recv());
        }));
    }
    let batch_rx = std::sync::Arc::clone(&rx);
    app.batch.handle = Some(std::thread::spawn(move || {
        let _ = batch_rx.lock().map(|rx| rx.recv());
    }));

    let start = Instant::now();
    app.batch
        .shutdown_deadline
        .set(start + Duration::from_millis(200))
        .expect("期限は未設定");
    drop(app);
    // 4 本それぞれに 200ms ずつ待つと 800ms 以上かかる
    assert!(start.elapsed() < Duration::from_millis(500));

    // 解放してスレッドを終わらせる
    drop(tx);
}
