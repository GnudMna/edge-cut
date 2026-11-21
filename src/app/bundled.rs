//! 配布物に同梱した文書 (利用者向けドキュメント・ライセンス) の探索と起動
//!
//! 同梱先は OS ごとの配布レイアウトに従い、実行ファイルの位置から求める。
//! 起動は OS の既定のアプリに任せ、依存クレートは追加しない。

use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

// ======================================================================
// 同梱文書
// ======================================================================
/// 配布物に同梱した文書
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Bundled {
    /// 利用者向けドキュメント (mdBook の HTML)
    Docs,
    /// このアプリのライセンス
    License,
    /// 依存クレートなど第三者のライセンス
    ThirdPartyNotices,
}

impl Bundled {
    /// メニューに並べる順
    pub(super) const ALL: [Self; 3] = [Self::Docs, Self::License, Self::ThirdPartyNotices];

    /// メニューの表示名
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Docs => "📖 ドキュメント",
            Self::License => "📄 ライセンス",
            Self::ThirdPartyNotices => "📄 第三者ライセンス",
        }
    }

    /// ホバー時の説明
    pub(super) fn description(self) -> &'static str {
        match self {
            Self::Docs => "利用者向けドキュメントを既定のブラウザで開く",
            Self::License => "このアプリのライセンスを開く",
            Self::ThirdPartyNotices => "使用しているオープンソースソフトウェアのライセンスを開く",
        }
    }

    /// 状態表示などで使う、記号を除いた名前
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Docs => "ドキュメント",
            Self::License => "ライセンス",
            Self::ThirdPartyNotices => "第三者ライセンス",
        }
    }

    /// 同梱先ディレクトリからの相対パス (区切り文字は OS に依存させない)
    fn components(self) -> &'static [&'static str] {
        match self {
            Self::Docs => &["docs", "index.html"],
            Self::License => &["LICENSE"],
            Self::ThirdPartyNotices => &["THIRD_PARTY_NOTICES"],
        }
    }

    /// HTML か (それ以外は拡張子のないテキスト)
    fn is_html(self) -> bool {
        self == Self::Docs
    }

    /// 同梱されている文書のパスを返す
    ///
    /// # Returns
    /// * `Option<PathBuf>` - 文書のパス。`cargo run` など配布物の外で動いている場合は `None`
    pub(super) fn locate(self) -> Option<PathBuf> {
        let exe = env::current_exe().ok()?;
        self.locate_in(&resource_dir(exe.parent()?)?)
    }

    /// 同梱先ディレクトリの中から文書を探す
    ///
    /// # Arguments
    /// * `resource_dir` - 同梱先ディレクトリ
    ///
    /// # Returns
    /// * `Option<PathBuf>` - 文書のパス。ファイルが無い場合は `None`
    fn locate_in(self, resource_dir: &Path) -> Option<PathBuf> {
        let path = self
            .components()
            .iter()
            .fold(resource_dir.to_path_buf(), |dir, part| dir.join(part));
        path.is_file().then_some(path)
    }

    /// 文書を OS の既定のアプリで開く
    ///
    /// 起動後は待たず、終了待ちは別スレッドで行う (ゾンビプロセスを残さないため)
    ///
    /// # Returns
    /// * `Result<(), String>` - 失敗時は利用者に見せるメッセージ
    pub(super) fn open(self) -> Result<(), String> {
        let path = self
            .locate()
            .ok_or("配布物に同梱されたファイルが見つからない")?;
        let mut child = open_command(&path, self.is_html())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e: io::Error| format!("起動に失敗: {e}"))?;
        thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

// ======================================================================
// 同梱先と起動コマンド
// ======================================================================
/// 実行ファイルのあるディレクトリから、文書の同梱先ディレクトリを求める
///
/// Windows は `<インストール先>\bin\EdgeCut.exe` に対する `<インストール先>`、
/// macOS は `Contents/MacOS` に対する `Contents/Resources`、Linux は実行ファイルと同じ場所
///
/// # Arguments
/// * `exe_dir` - 実行ファイルのあるディレクトリ
///
/// # Returns
/// * `Option<PathBuf>` - 同梱先ディレクトリ。親ディレクトリを辿れない場合は `None`
#[cfg(windows)]
fn resource_dir(exe_dir: &Path) -> Option<PathBuf> {
    exe_dir.parent().map(Path::to_path_buf)
}

/// 同梱先ディレクトリを求める (macOS)
#[cfg(target_os = "macos")]
fn resource_dir(exe_dir: &Path) -> Option<PathBuf> {
    exe_dir.parent().map(|contents| contents.join("Resources"))
}

/// 同梱先ディレクトリを求める (Linux など)
#[cfg(not(any(windows, target_os = "macos")))]
#[allow(clippy::unnecessary_wraps)]
fn resource_dir(exe_dir: &Path) -> Option<PathBuf> {
    Some(exe_dir.to_path_buf())
}

/// 文書を開くコマンドを組み立てる
///
/// 拡張子のないテキストは関連付けが無いため、Windows ではメモ帳、macOS では `open -t` で開く
///
/// # Arguments
/// * `path` - 開く文書のパス
/// * `is_html` - HTML か (`false` はテキスト)
///
/// # Returns
/// * `Command` - 未起動のコマンド
#[cfg(windows)]
fn open_command(path: &Path, is_html: bool) -> Command {
    let mut command = Command::new(if is_html {
        "explorer.exe"
    } else {
        "notepad.exe"
    });
    command.arg(path);
    command
}

/// 文書を開くコマンドを組み立てる (macOS)
#[cfg(target_os = "macos")]
fn open_command(path: &Path, is_html: bool) -> Command {
    let mut command = Command::new("open");
    if !is_html {
        command.arg("-t");
    }
    command.arg(path);
    command
}

/// 文書を開くコマンドを組み立てる (Linux など)
#[cfg(not(any(windows, target_os = "macos")))]
fn open_command(path: &Path, _is_html: bool) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(path);
    command
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// 同梱先に文書がある場合は、そのパスを返すこと
    #[test]
    fn locate_in_finds_existing_documents() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("docs")).unwrap();
        fs::write(dir.path().join("docs").join("index.html"), "").unwrap();
        fs::write(dir.path().join("LICENSE"), "").unwrap();
        fs::write(dir.path().join("THIRD_PARTY_NOTICES"), "").unwrap();

        for item in Bundled::ALL {
            let path = item.locate_in(dir.path()).unwrap();
            assert!(path.starts_with(dir.path()), "{item:?}");
            assert!(path.is_file(), "{item:?}");
        }
    }

    /// 同梱先に文書が無い場合は `None` を返すこと
    ///
    /// ディレクトリだけが存在する場合も、ファイルではないため見つからない扱いにする
    #[test]
    fn locate_in_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("docs").join("index.html")).unwrap();

        for item in Bundled::ALL {
            assert_eq!(item.locate_in(dir.path()), None, "{item:?}");
        }
    }

    /// 実行ファイルの位置から、OS の配布レイアウトに沿った同梱先を求めること
    #[test]
    fn resource_dir_follows_distribution_layout() {
        #[cfg(windows)]
        assert_eq!(
            resource_dir(Path::new(r"C:\app\edge-cut\bin")),
            Some(PathBuf::from(r"C:\app\edge-cut"))
        );
        #[cfg(target_os = "macos")]
        assert_eq!(
            resource_dir(Path::new("/Applications/EdgeCut.app/Contents/MacOS")),
            Some(PathBuf::from(
                "/Applications/EdgeCut.app/Contents/Resources"
            ))
        );
        #[cfg(not(any(windows, target_os = "macos")))]
        assert_eq!(
            resource_dir(Path::new("/home/u/.local/share/edge-cut")),
            Some(PathBuf::from("/home/u/.local/share/edge-cut"))
        );
    }

    /// 開くコマンドの引数に文書のパスを渡すこと
    #[test]
    fn open_command_passes_path() {
        let path = Path::new("dir with space").join("LICENSE");
        for is_html in [true, false] {
            let command = open_command(&path, is_html);
            assert_eq!(
                command.get_args().last(),
                Some(path.as_os_str()),
                "is_html={is_html}"
            );
        }
    }

    /// 同梱されていない文書を開こうとした場合は、メッセージ付きの失敗を返すこと
    ///
    /// テスト実行ファイルは配布物の外にあるため、どの文書も見つからない
    #[test]
    fn open_fails_outside_distribution() {
        for item in Bundled::ALL {
            if item.locate().is_none() {
                assert!(item.open().is_err(), "{item:?}");
            }
        }
    }
}
