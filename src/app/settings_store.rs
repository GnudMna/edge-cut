//! 設定ファイルの読み書き
//!
//! 設定は `%APPDATA%\EdgeCut\settings.json` (ポータブル版は `<実行フォルダ>\data\settings.json`) に JSON で保存する。
//! 設定ファイルが無い場合は、egui 版が保存した `%APPDATA%\EdgeCut\data\app.ron` から引き継ぐ

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::state::Settings;
use crate::install_layout;

/// 設定ファイル名
const SETTINGS_FILE_NAME: &str = "settings.json";

/// 読み込めなかった設定ファイルの退避先の拡張子
const BACKUP_EXTENSION: &str = "json.bak";

/// egui 版の保存ファイル (`%APPDATA%\EdgeCut` からの相対パス)
const LEGACY_FILE: [&str; 2] = ["data", "app.ron"];

/// egui 版の保存ファイルで設定を格納していたキー
const LEGACY_SETTINGS_KEY: &str = "settings";

// ======================================================================
// 保存先
// ======================================================================
/// 設定ファイルと egui 版の保存ファイルの場所
#[derive(Debug, Clone)]
pub(super) struct SettingsStore {
    /// 設定ファイル
    path: PathBuf,
    /// egui 版の保存ファイル (設定ファイルが無いときだけ読む)
    legacy_path: PathBuf,
}

impl SettingsStore {
    /// 既定の場所 (`%APPDATA%\EdgeCut`、ポータブル版は `<実行フォルダ>\data`) の設定ファイルを扱う
    ///
    /// # Returns
    /// * `Option<Self>` - 保存先を決められない場合は `None` (設定を保存しない)
    pub(super) fn locate() -> Option<Self> {
        Some(Self::in_dir(&install_layout::settings_dir()?))
    }

    /// 指定フォルダの設定ファイルを扱う
    ///
    /// # Arguments
    /// * `dir` - 設定ファイルを置くフォルダ
    ///
    /// # Returns
    /// * `Self` - `dir` 配下を読み書きする `SettingsStore`
    pub(super) fn in_dir(dir: &Path) -> Self {
        Self {
            path: dir.join(SETTINGS_FILE_NAME),
            legacy_path: LEGACY_FILE.iter().fold(dir.to_path_buf(), |p, c| p.join(c)),
        }
    }

    /// 保存済みの設定を読み込む
    ///
    /// 設定ファイルが無ければ egui 版の保存ファイルから引き継ぐ。どちらも無い場合は既定の設定を返す。
    /// 設定ファイルが読めない・壊れている場合も既定の設定を返す (保存データの不備で起動できなく
    /// ならないようにする) が、次の保存で上書きされないよう `.bak` へ退避し、理由を通知文で返す
    ///
    /// # Returns
    /// * `(Settings, Option<String>)` - 読み込んだ設定 (正規化前) と、利用者へ知らせる通知文
    pub(super) fn load(&self) -> (Settings, Option<String>) {
        match fs::read_to_string(&self.path) {
            Ok(text) => match parse_settings(&text) {
                Some(settings) => (settings, None),
                None => (Settings::default(), Some(self.set_aside())),
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let settings = fs::read_to_string(&self.legacy_path)
                    .ok()
                    .and_then(|text| parse_legacy_settings(&text))
                    .unwrap_or_default();
                (settings, None)
            }
            Err(_) => (Settings::default(), Some(self.set_aside())),
        }
    }

    /// 読み込めなかった設定ファイルを `.bak` へ退避する
    ///
    /// 既存の `.bak` は置き換える
    ///
    /// # Returns
    /// * `String` - 退避の結果を伝える通知文
    fn set_aside(&self) -> String {
        let backup = self.path.with_extension(BACKUP_EXTENSION);
        match fs::rename(&self.path, &backup) {
            Ok(()) => format!(
                "設定ファイルを読み込めなかったため、初期値で起動した (元のファイルは {} に退避)",
                backup.display()
            ),
            Err(_) => "設定ファイルを読み込めなかったため、初期値で起動した".into(),
        }
    }

    /// 設定を保存する
    ///
    /// 書きかけのファイルが残らないよう、一時ファイルへ書いてディスクへ同期してから置き換える
    ///
    /// # Arguments
    /// * `settings` - 保存する設定
    ///
    /// # Errors
    /// フォルダの作成・書き込み・同期・置き換えに失敗した場合
    pub(super) fn save(&self, settings: &Settings) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(settings).map_err(io::Error::other)?;
        let tmp = self.path.with_extension("json.tmp");
        let written = fs::File::create(&tmp).and_then(|mut file| {
            file.write_all(json.as_bytes())?;
            file.sync_all()
        });
        let result = written.and_then(|()| fs::rename(&tmp, &self.path));
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}

// ======================================================================
// 解析
// ======================================================================
/// 設定ファイル (JSON) を解析する
///
/// # Arguments
/// * `text` - 設定ファイルの内容
///
/// # Returns
/// * `Option<Settings>` - 解析できなければ `None`
pub(super) fn parse_settings(text: &str) -> Option<Settings> {
    serde_json::from_str(text).ok()
}

/// egui 版の保存ファイル (RON) から設定を取り出す
///
/// 保存ファイルはキーごとに RON 文字列を持つ表で、設定は `settings` キーにある
///
/// # Arguments
/// * `text` - 保存ファイルの内容
///
/// # Returns
/// * `Option<Settings>` - 設定が無い・解析できなければ `None`
pub(super) fn parse_legacy_settings(text: &str) -> Option<Settings> {
    let map: HashMap<String, String> = ron::from_str(text).ok()?;
    parse_legacy_value(map.get(LEGACY_SETTINGS_KEY)?)
}

/// egui 版が保存した設定の値 (RON) を解析する
///
/// # Arguments
/// * `value` - `settings` キーの値
///
/// # Returns
/// * `Option<Settings>` - 解析できなければ `None`
pub(super) fn parse_legacy_value(value: &str) -> Option<Settings> {
    ron::from_str(value).ok()
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// 保存した設定を読み戻せること
    #[test]
    fn save_then_load_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SettingsStore::in_dir(&tmp.path().join("EdgeCut"));
        let settings = Settings {
            tolerance: 12,
            panel_width: 400,
            ..Settings::default()
        };
        store.save(&settings).unwrap();

        let (loaded, notice) = store.load();
        assert!(notice.is_none());
        assert_eq!(loaded.tolerance, 12);
        assert_eq!(loaded.panel_width, 400);
    }

    /// 設定ファイルが無い場合は egui 版の保存ファイルから引き継ぐこと
    #[test]
    fn load_migrates_legacy_ron() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        fs::create_dir_all(&data).unwrap();
        fs::write(
            data.join("app.ron"),
            r#"{"egui": "(options:())", "settings": "(tolerance: 9, dark_mode: false)"}"#,
        )
        .unwrap();

        let loaded = SettingsStore::in_dir(tmp.path()).load().0;
        assert_eq!(loaded.tolerance, 9);
        assert!(!loaded.dark_mode);
    }

    /// 設定ファイルがあれば egui 版の保存ファイルは読まないこと
    #[test]
    fn load_prefers_settings_file_over_legacy() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SettingsStore::in_dir(tmp.path());
        store
            .save(&Settings {
                tolerance: 3,
                ..Settings::default()
            })
            .unwrap();
        let data = tmp.path().join("data");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("app.ron"), r#"{"settings": "(tolerance: 9)"}"#).unwrap();

        assert_eq!(store.load().0.tolerance, 3);
    }

    /// 保存データが無い場合は通知なしで既定の設定になること
    #[test]
    fn load_without_files_returns_default_silently() {
        let tmp = tempfile::tempdir().unwrap();
        let (loaded, notice) = SettingsStore::in_dir(tmp.path()).load();
        assert_eq!(loaded.tolerance, Settings::default().tolerance);
        assert!(notice.is_none());
    }

    /// 壊れた設定ファイルは `.bak` へ退避し、通知付きで既定の設定になること
    ///
    /// 一部の値だけが範囲外の場合も同様に扱い、退避後の保存で元の内容を失わない
    #[test]
    fn load_sets_aside_broken_file() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SettingsStore::in_dir(tmp.path());
        let path = tmp.path().join(SETTINGS_FILE_NAME);
        let backup = tmp.path().join("settings.json.bak");
        let broken = r#"{"tolerance": 5, "jpeg_quality": 300}"#;
        fs::write(&path, broken).unwrap();

        let (loaded, notice) = store.load();
        // 既定値になり、利用者へ知らせる
        assert_eq!(loaded.tolerance, Settings::default().tolerance);
        assert!(notice.is_some());
        // 元の内容は退避先に残り、設定ファイルは無くなる
        assert!(!path.exists());
        assert_eq!(fs::read_to_string(&backup).unwrap(), broken);

        // 続けて保存しても退避したファイルは変わらない
        store.save(&loaded).unwrap();
        assert_eq!(fs::read_to_string(&backup).unwrap(), broken);
        assert!(store.load().1.is_none());
    }

    /// egui 版の保存ファイルに設定が無い場合は `None` を返すこと
    #[test]
    fn parse_legacy_settings_without_settings_key() {
        assert!(parse_legacy_settings(r#"{"egui": "()"}"#).is_none());
        assert!(parse_legacy_settings("not ron").is_none());
    }
}
