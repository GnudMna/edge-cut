use std::collections::HashMap;

use eframe::Storage;

use super::*;
use crate::consts::{JPEG_QUALITY_DEFAULT, PADDING_MAX, TOL_MAX, WEBP_QUALITY_DEFAULT};

/// `App::default()` が画像なしで初期化されること
#[test]
fn new_has_no_images() {
    let app = App::default();
    assert!(app.image.original.is_none());
    assert!(app.image.orig_tex.is_none());
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

/// `App::default()` がコンパクトモードをデフォルトで無効にすること
#[test]
fn new_compact_mode_disabled_by_default() {
    let app = App::default();
    assert!(!app.prefs.compact_mode);
}

/// `App::default()` がステータスメッセージを適切に初期化すること
#[test]
fn new_status_message_is_not_empty() {
    let app = App::default();
    assert!(!app.status.is_empty());
}

/// `App::default()` が自動保存ディレクトリを適切に初期化すること
#[test]
fn new_auto_save_dir_is_non_empty_path() {
    let app = App::default();
    assert!(!app.prefs.auto_save_dir.as_os_str().is_empty());
}
/// `save_settings` が範囲外の値を保存前に正規化すること
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
/// `save_settings` が手動背景色を保存しないこと
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
    let mut storage = TestStorage::default();
    storage.set_string(
        SETTINGS_KEY,
        "(tolerance: 5, bg_override: Some((40, 50, 60, 200)))".to_owned(),
    );

    let app = App::new(Some(&storage));
    assert_eq!(app.prefs.tolerance, 5);
    assert_eq!(app.prefs.bg_override, None);
    assert_eq!(app.background.bg_color, None);
}

/// 保存データに一部のフィールドしかなくても、残りを既定値で補って読み込めること
#[test]
fn app_new_fills_missing_settings_fields_with_defaults() {
    let mut storage = TestStorage::default();
    storage.set_string(
        SETTINGS_KEY,
        "(tolerance: 42, compact_mode: true)".to_owned(),
    );

    let app = App::new(Some(&storage));
    let defaults = Settings::default();
    // 保存データにあるフィールドは復元される
    assert_eq!(app.prefs.tolerance, 42);
    assert!(app.prefs.compact_mode);
    // 無いフィールドは既定値になる
    assert_eq!(app.prefs.padding, defaults.padding);
    assert_eq!(app.prefs.dark_mode, defaults.dark_mode);
    assert_eq!(app.prefs.jpeg_quality, JPEG_QUALITY_DEFAULT);
    assert_eq!(app.prefs.webp_quality, WEBP_QUALITY_DEFAULT);
}

/// 保存データに未知のフィールドがあっても、他の設定を読み込めること
#[test]
fn app_new_ignores_unknown_settings_fields() {
    let mut storage = TestStorage::default();
    storage.set_string(SETTINGS_KEY, "(tolerance: 7, removed_field: 1)".to_owned());

    let app = App::new(Some(&storage));
    assert_eq!(app.prefs.tolerance, 7);
}
// ======================================================================
// ヘルパー
// ======================================================================
/// メモリ上のテスト用ストレージ
#[derive(Default)]
struct TestStorage(HashMap<String, String>);

impl eframe::Storage for TestStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }

    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }

    fn flush(&mut self) {}
}

/// `save_settings` で保存した設定をそのまま読み戻す
fn save_and_load(app: &App) -> Settings {
    let mut storage = TestStorage::default();
    app.save_settings(&mut storage);
    eframe::get_value(&storage, SETTINGS_KEY).expect("保存済みの設定")
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
    let mut app = App::new(None);
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
