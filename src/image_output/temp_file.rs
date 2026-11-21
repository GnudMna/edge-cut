//! 保存用の一時ファイル

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// 保存用の一時ファイル名が衝突した場合に試す回数
const TEMP_FILE_ATTEMPTS: u32 = 100;

/// 一時ファイル名の接頭辞
const TEMP_PREFIX: &str = ".edgecut-";

/// 一時ファイル名の拡張子
const TEMP_SUFFIX: &str = ".tmp";

/// 中断で残った一時ファイルとみなす最終更新からの経過時間
///
/// 他プロセスが保存中の一時ファイルを消さないよう、保存に要する時間より十分長くする
const STALE_TEMP_AGE: Duration = Duration::from_hours(1);

// ======================================================================
// 古い一時ファイルの掃除
// ======================================================================
/// 掃除済みのディレクトリ (フォルダ内の走査を保存のたびに繰り返さないため)
static CLEANED_DIRS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

/// プロセス内で初めて保存するディレクトリに限り、古い一時ファイルを掃除する
///
/// # Arguments
/// * `dir` - 保存先のディレクトリ
fn cleanup_stale_once(dir: &Path) {
    let cleaned = CLEANED_DIRS.get_or_init(|| Mutex::new(HashSet::new()));
    let first = cleaned
        .lock()
        .is_ok_and(|mut set| set.insert(dir.to_path_buf()));
    if first {
        remove_stale(dir, STALE_TEMP_AGE);
    }
}

/// `dir` 直下の一時ファイルのうち、最終更新から `max_age` 以上経ったものを削除する
///
/// 強制終了や電源断で `Drop` が走らず残ったファイルの回収が目的のため、失敗は無視する
///
/// # Arguments
/// * `dir`     - 走査するディレクトリ
/// * `max_age` - 削除対象とする最終更新からの経過時間
fn remove_stale(dir: &Path, max_age: Duration) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(TEMP_PREFIX) || !name.ends_with(TEMP_SUFFIX) {
            continue;
        }
        let stale = entry
            .metadata()
            .ok()
            .filter(std::fs::Metadata::is_file)
            .and_then(|meta| meta.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= max_age);
        if stale {
            let _ = fs::remove_file(entry.path());
        }
    }
}

// ======================================================================
// 一時ファイル
// ======================================================================
/// 保存先と同じディレクトリに作り、書き込みとディスクへの同期を終えた一時ファイル
///
/// 書き込みを終えてから保存先の名前で公開するため、中断しても書きかけのファイルが保存先に残らない
/// 保存先へ移す前に破棄された場合は削除する
pub(super) struct TempFile {
    /// 一時ファイルのパス
    pub(super) path: PathBuf,
    /// rename で保存先へ移し済み (削除すべきファイルがない)
    persisted: bool,
}

impl TempFile {
    /// `target` と同じディレクトリに一時ファイルを作り、`bytes` を書き込んでディスクへ同期する
    ///
    /// rename が同一ボリューム内で完結するよう `target` と同じディレクトリに作る
    /// 書き込みや同期に失敗した場合は一時ファイルを削除する
    ///
    /// # Arguments
    /// * `target` - 最終的な保存先パス (一時ファイル名の生成に使う)
    /// * `bytes`  - 書き込む内容
    pub(super) fn write(target: &Path, bytes: &[u8]) -> io::Result<Self> {
        let dir = match target.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => Path::new("."),
        };
        if target.file_name().is_none() {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }

        cleanup_stale_once(dir);

        for n in 0..TEMP_FILE_ATTEMPTS {
            // 保存名が長くても 255 文字の制限を超えないよう、保存名を含めない固定長の名前にする
            let path = dir.join(format!("{TEMP_PREFIX}{}-{n}{TEMP_SUFFIX}", process::id()));

            let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            };
            // 保存先へ移した後に中身が欠けないよう、移す前にディスクへ反映させる
            let written = file.write_all(bytes).and_then(|()| file.sync_all());
            // Windows では開いたままのファイルを削除できないため、ガードを作る前に閉じる
            drop(file);
            let temp = Self {
                path,
                persisted: false,
            };
            return written.map(|()| temp);
        }
        Err(io::Error::from(io::ErrorKind::AlreadyExists))
    }

    /// 一時ファイルを `target` へ rename して置き換える (既存ファイルは上書き)
    ///
    /// 失敗した場合は一時ファイルを削除し、`target` は元のまま残す
    pub(super) fn replace(mut self, target: &Path) -> io::Result<()> {
        fs::rename(&self.path, target)?;
        self.persisted = true;
        Ok(())
    }

    /// `target` が存在しない場合のみ、一時ファイルの内容を `target` として公開する
    ///
    /// ハードリンクは既存ファイルを上書きせず存在確認と作成を同時に行うため、並行する保存とも衝突しない
    /// ハードリンクに対応しないファイルシステム (FAT 系など) では `reserve_and_replace` で代替する
    ///
    /// # Returns
    /// * `bool` - 公開した場合は `true`。`target` が既に存在する場合は `false` を返し、
    ///   別の名前で再試行できるよう一時ファイルを残す
    pub(super) fn persist_new(&mut self, target: &Path) -> io::Result<bool> {
        match fs::hard_link(&self.path, target) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(_) => self.reserve_and_replace(target),
        }
    }

    /// 空ファイルで `target` の名前を確保してから、一時ファイルで置き換える
    ///
    /// rename までに中断すると空ファイルが残りうるが、書きかけの画像は残らない
    /// rename に失敗した場合は確保した空ファイルを削除する
    ///
    /// # Returns
    /// * `bool` - 置き換えた場合は `true`、`target` が既に存在する場合は `false`
    fn reserve_and_replace(&mut self, target: &Path) -> io::Result<bool> {
        match OpenOptions::new().write(true).create_new(true).open(target) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
            Err(e) => return Err(e),
        }
        if let Err(e) = fs::rename(&self.path, target) {
            let _ = fs::remove_file(target);
            return Err(e);
        }
        self.persisted = true;
        Ok(true)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        // rename 後の名前は別の保存が一時ファイルとして再利用しうるため消さない
        if !self.persisted {
            let _ = fs::remove_file(&self.path);
        }
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// ハードリンク非対応時の代替経路で、既存ファイルを上書きせずに新規作成できること
    #[test]
    fn temp_file_reserve_and_replace_creates_new_file_only() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let existing = dir.join("existing.png");
        fs::write(&existing, b"keep").expect("placeholder");
        let target = dir.join("new.png");

        let mut staged = TempFile::write(&target, b"data").expect("一時ファイル");
        // 既存ファイルは上書きせず、一時ファイルは再試行用に残す
        assert!(!staged.reserve_and_replace(&existing).expect("既存"));
        assert_eq!(fs::read(&existing).expect("read"), b"keep");
        assert!(staged.path.is_file());

        // 空いている名前には一時ファイルの内容で作成する
        assert!(staged.reserve_and_replace(&target).expect("新規"));
        assert_eq!(fs::read(&target).expect("read"), b"data");
        drop(staged);
        assert_eq!(fs::read_dir(dir).expect("read_dir").count(), 2);
    }

    /// 保存名が 255 文字に近い場合も一時ファイルを作成できること
    #[test]
    fn temp_file_supports_long_target_name() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join(format!("{}.png", "a".repeat(240)));

        let staged = TempFile::write(&target, b"data").expect("一時ファイル");
        staged.replace(&target).expect("replace");
        assert_eq!(fs::read(&target).expect("read"), b"data");
    }

    /// 古い一時ファイルだけを削除し、新しい一時ファイルや無関係なファイルは残すこと
    #[test]
    fn remove_stale_deletes_only_old_temp_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let old = dir.join(".edgecut-1-0.tmp");
        let fresh = dir.join(".edgecut-2-0.tmp");
        let other = dir.join("photo.tmp");
        for path in [&old, &fresh, &other] {
            fs::write(path, b"x").expect("write");
        }
        // 2 時間前の更新時刻にして、掃除対象の経過時間を超えさせる
        let two_hours_ago = SystemTime::now() - Duration::from_hours(2);
        fs::File::options()
            .write(true)
            .open(&old)
            .and_then(|f| f.set_modified(two_hours_ago))
            .expect("set_modified");
        fs::File::options()
            .write(true)
            .open(&other)
            .and_then(|f| f.set_modified(two_hours_ago))
            .expect("set_modified");

        remove_stale(dir, STALE_TEMP_AGE);

        assert!(!old.exists());
        assert!(fresh.exists());
        assert!(other.exists());
    }

    /// 公開せずに破棄した一時ファイルは削除されること
    #[test]
    fn temp_file_removed_on_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = TempFile::write(&tmp.path().join("out.png"), b"data").expect("一時ファイル");
        assert!(staged.path.is_file());
        drop(staged);
        assert_eq!(fs::read_dir(tmp.path()).expect("read_dir").count(), 0);
    }
}
