//! 保存用の一時ファイル

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;

/// 保存用の一時ファイル名が衝突した場合に試す回数
const TEMP_FILE_ATTEMPTS: u32 = 100;

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
        let name = target
            .file_name()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;

        for n in 0..TEMP_FILE_ATTEMPTS {
            let mut temp_name = OsString::from(".");
            temp_name.push(name);
            temp_name.push(format!(".{}.{n}.tmp", process::id()));
            let path = dir.join(temp_name);

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
