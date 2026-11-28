//! ポータブル版のファイルの差し替え
//!
//! 展開済みの配布物を実行フォルダへコピーする。上書き・削除するファイルは実行フォルダ内の退避フォルダへ移し、
//! 途中で失敗したら差し替え前の状態へ戻す。利用者のデータ (`data`) は差し替えの対象にしない

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::super::error::UpdateError;
use crate::install_layout::{
    APP_EXE_NAME, PORTABLE_DATA_DIR_NAME, UPDATER_EXE_NAME, is_portable_dir,
};

// ======================================================================
// 定数
// ======================================================================
/// 差し替え中に旧ファイルを退避する、実行フォルダ直下のフォルダ名
const BACKUP_DIR_NAME: &str = ".edge-cut-update-backup";

// ======================================================================
// 差し替え
// ======================================================================
/// 展開済みの配布物を実行フォルダへ反映する
///
/// 本体の終了後に呼ぶ。直下のファイルは上書きと追加だけ行い、利用者が置いたファイルは消さない
/// 配布物のサブフォルダ (`docs` など) からは、新しい版に無いファイルを取り除く
///
/// # Arguments
/// * `source_dir` - 展開ルート (新しい `EdgeCut.exe` があるフォルダ)
/// * `install_dir` - 差し替え先 (本体の実行フォルダ)
///
/// # Returns
/// * `Result<(), UpdateError>` - 成功時 `Ok(())`。失敗時は差し替え前の状態へ戻す
pub(crate) fn install_portable(source_dir: &Path, install_dir: &Path) -> Result<(), UpdateError> {
    let (source_dir, install_dir) = validate_dirs(source_dir, install_dir)?;
    let plan = plan_replace(&source_dir, &install_dir).map_err(|err| replace_failed("", &err))?;

    let backup_dir = install_dir.join(BACKUP_DIR_NAME);
    remove_dir_if_exists(&backup_dir).map_err(|err| replace_failed(BACKUP_DIR_NAME, &err))?;

    let mut journal = Journal::default();
    match apply_plan(&plan, &source_dir, &install_dir, &backup_dir, &mut journal) {
        Ok(()) => {
            let _ = fs::remove_dir_all(&backup_dir);
            Ok(())
        }
        Err(err) => {
            if journal.rollback() {
                let _ = fs::remove_dir_all(&backup_dir);
                Err(err)
            } else {
                Err(UpdateError::RollbackFailed {
                    backup: backup_dir.display().to_string(),
                })
            }
        }
    }
}

// ======================================================================
// 検証
// ======================================================================
/// 差し替え元・先がポータブル版の前提を満たすか確かめ、正規化したパスを返す
///
/// 差し替え先は `portable` マーカーと `EdgeCut.exe` を持つ既存のフォルダに限る
/// どちらかがもう一方の中にある場合は、差し替えでファイルを壊すため拒否する
fn validate_dirs(source_dir: &Path, install_dir: &Path) -> Result<(PathBuf, PathBuf), UpdateError> {
    let source = source_dir
        .canonicalize()
        .map_err(|_| UpdateError::PortableInvalid)?;
    let install = install_dir
        .canonicalize()
        .map_err(|_| UpdateError::InstallDirInvalid)?;

    let source_complete = [APP_EXE_NAME, UPDATER_EXE_NAME]
        .iter()
        .all(|name| source.join(name).is_file())
        && is_portable_dir(&source);
    if !source_complete {
        return Err(UpdateError::PortableInvalid);
    }
    let install_ok = install.join(APP_EXE_NAME).is_file() && is_portable_dir(&install);
    if !install_ok || source.starts_with(&install) || install.starts_with(&source) {
        return Err(UpdateError::InstallDirInvalid);
    }
    Ok((source, install))
}

// ======================================================================
// 計画
// ======================================================================
/// 差し替えで行うファイル操作
#[derive(Debug, Default)]
struct ReplacePlan {
    /// コピーするファイル (配布物ルートからの相対パス)
    copy: Vec<PathBuf>,
    /// 新しい版に無いため取り除くファイル (実行フォルダからの相対パス)
    remove: Vec<PathBuf>,
}

/// 配布物と実行フォルダを比べて、コピーと削除の対象を決める
fn plan_replace(source_dir: &Path, install_dir: &Path) -> io::Result<ReplacePlan> {
    let mut plan = ReplacePlan::default();
    for entry in fs::read_dir(source_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if is_reserved_name(&name.to_string_lossy()) {
            continue;
        }
        let rel = PathBuf::from(&name);
        if entry.file_type()?.is_dir() {
            let mut files = Vec::new();
            collect_files(&entry.path(), &rel, &mut files)?;
            let keep: HashSet<_> = files.iter().cloned().collect();
            let installed = install_dir.join(&rel);
            if installed.is_dir() {
                let mut old = Vec::new();
                collect_files(&installed, &rel, &mut old)?;
                plan.remove
                    .extend(old.into_iter().filter(|path| !keep.contains(path)));
            }
            plan.copy.extend(files);
        } else {
            plan.copy.push(rel);
        }
    }
    plan.copy.sort();
    plan.remove.sort();
    Ok(plan)
}

/// フォルダ配下のファイルを相対パスで集める (リンクはたどらない)
fn collect_files(dir: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let child = rel.join(entry.file_name());
        if kind.is_dir() {
            collect_files(&entry.path(), &child, out)?;
        } else if kind.is_file() {
            out.push(child);
        }
    }
    Ok(())
}

/// 差し替えの対象にしない直下の名前 (利用者のデータと退避フォルダ)
fn is_reserved_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(PORTABLE_DATA_DIR_NAME) || name.eq_ignore_ascii_case(BACKUP_DIR_NAME)
}

// ======================================================================
// 適用と巻き戻し
// ======================================================================
/// 計画どおりにファイルを退避・コピーする
fn apply_plan(
    plan: &ReplacePlan,
    source_dir: &Path,
    install_dir: &Path,
    backup_dir: &Path,
    journal: &mut Journal,
) -> Result<(), UpdateError> {
    for rel in &plan.remove {
        journal
            .move_to_backup(&install_dir.join(rel), &backup_dir.join(rel))
            .map_err(|err| replace_failed(&rel.to_string_lossy(), &err))?;
    }
    for rel in &plan.copy {
        let dest = install_dir.join(rel);
        let result = journal
            .move_to_backup(&dest, &backup_dir.join(rel))
            .and_then(|()| journal.copy_new(&source_dir.join(rel), &dest));
        result.map_err(|err| replace_failed(&rel.to_string_lossy(), &err))?;
    }
    Ok(())
}

/// 差し替えで行った操作の記録 (失敗時に逆順で戻す)
#[derive(Debug, Default)]
struct Journal {
    /// 作ったフォルダ
    created_dirs: Vec<PathBuf>,
    /// 退避したファイル (元の場所, 退避先)
    moved: Vec<(PathBuf, PathBuf)>,
    /// コピーしたファイル
    copied: Vec<PathBuf>,
}

impl Journal {
    /// ファイルがあれば退避フォルダへ移す
    fn move_to_backup(&mut self, path: &Path, backup: &Path) -> io::Result<()> {
        if !path.exists() {
            return Ok(());
        }
        if let Some(parent) = backup.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(path, backup)?;
        self.moved.push((path.to_path_buf(), backup.to_path_buf()));
        Ok(())
    }

    /// 新しいファイルをコピーする (無いフォルダは作る)
    fn copy_new(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        if let Some(parent) = to.parent() {
            self.create_dirs(parent)?;
        }
        fs::copy(from, to)?;
        self.copied.push(to.to_path_buf());
        Ok(())
    }

    /// 無いフォルダを親から順に作り、作ったものを記録する
    fn create_dirs(&mut self, dir: &Path) -> io::Result<()> {
        if dir.is_dir() {
            return Ok(());
        }
        if let Some(parent) = dir.parent() {
            self.create_dirs(parent)?;
        }
        fs::create_dir(dir)?;
        self.created_dirs.push(dir.to_path_buf());
        Ok(())
    }

    /// 記録した操作を逆順に戻す
    ///
    /// # Returns
    /// * `bool` - 退避したファイルをすべて元へ戻せた場合は `true`
    fn rollback(&mut self) -> bool {
        for path in self.copied.drain(..).rev() {
            let _ = fs::remove_file(path);
        }
        let mut restored = true;
        for (path, backup) in self.moved.drain(..).rev() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if fs::rename(&backup, &path).is_err() {
                restored = false;
            }
        }
        for dir in self.created_dirs.drain(..).rev() {
            let _ = fs::remove_dir(dir);
        }
        restored
    }
}

/// フォルダがあれば削除する
fn remove_dir_if_exists(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

/// 差し替え失敗のエラーを作る
fn replace_failed(path: &str, err: &io::Error) -> UpdateError {
    UpdateError::ReplaceFailed {
        detail: format!("{path}: {err}"),
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;

    use super::*;
    use crate::install_layout::PORTABLE_MARKER_FILE;

    /// ファイルを書く (親フォルダも作る)
    fn write(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        fs::write(path, body).expect("write");
    }

    /// ファイルの内容を読む
    fn read(path: &Path) -> String {
        fs::read_to_string(path).expect("read")
    }

    /// 指定した版のポータブル版フォルダを作る
    fn portable(dir: &Path, version: &str) {
        write(&dir.join(APP_EXE_NAME), &format!("app {version}"));
        write(&dir.join(UPDATER_EXE_NAME), &format!("updater {version}"));
        write(&dir.join(PORTABLE_MARKER_FILE), "");
        write(
            &dir.join("docs").join("index.html"),
            &format!("docs {version}"),
        );
    }

    /// 新しい版のファイルで差し替え、利用者のデータとファイルを残すこと
    ///
    /// サブフォルダからは新しい版に無いファイルを取り除く
    #[test]
    fn install_portable_replaces_files_and_keeps_data() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (source, install) = (tmp.path().join("new"), tmp.path().join("app"));
        portable(&source, "2");
        write(&source.join("docs").join("new.html"), "new page");
        portable(&install, "1");
        write(&install.join("docs").join("old.html"), "old page");
        write(&install.join("data").join("settings.json"), "user");
        write(&install.join("memo.txt"), "user file");

        install_portable(&source, &install).expect("install");

        // 新しい版のファイルになる
        assert_eq!(read(&install.join(APP_EXE_NAME)), "app 2");
        assert_eq!(read(&install.join(UPDATER_EXE_NAME)), "updater 2");
        assert_eq!(read(&install.join("docs").join("index.html")), "docs 2");
        assert_eq!(read(&install.join("docs").join("new.html")), "new page");
        // 新しい版に無いサブフォルダのファイルは取り除く
        assert!(!install.join("docs").join("old.html").exists());
        // 利用者のデータと直下のファイルは残す
        assert_eq!(read(&install.join("data").join("settings.json")), "user");
        assert_eq!(read(&install.join("memo.txt")), "user file");
        // 退避フォルダは残さない
        assert!(!install.join(BACKUP_DIR_NAME).exists());
    }

    /// 配布物の `data` で利用者のデータを上書きしないこと
    #[test]
    fn install_portable_skips_source_data() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (source, install) = (tmp.path().join("new"), tmp.path().join("app"));
        portable(&source, "2");
        write(&source.join("data").join("settings.json"), "packaged");
        portable(&install, "1");
        write(&install.join("data").join("settings.json"), "user");

        install_portable(&source, &install).expect("install");
        assert_eq!(read(&install.join("data").join("settings.json")), "user");
    }

    /// 途中で失敗したら差し替え前の状態へ戻すこと
    #[test]
    fn install_portable_rolls_back_on_failure() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (source, install) = (tmp.path().join("new"), tmp.path().join("app"));
        portable(&source, "2");
        write(&source.join("docs").join("sub").join("added.html"), "added");
        portable(&install, "1");

        // 共有を許さずに開いたファイルは移動できないため、最後に処理するマーカーで失敗させる
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(install.join(PORTABLE_MARKER_FILE))
            .expect("lock");
        let result = install_portable(&source, &install);
        drop(locked);

        assert!(
            matches!(result, Err(UpdateError::ReplaceFailed { .. })),
            "{result:?}"
        );
        // 先に差し替えたファイルも元に戻り、追加したフォルダは残らない
        assert_eq!(read(&install.join(APP_EXE_NAME)), "app 1");
        assert_eq!(read(&install.join(UPDATER_EXE_NAME)), "updater 1");
        assert_eq!(read(&install.join("docs").join("index.html")), "docs 1");
        assert!(!install.join("docs").join("sub").exists());
        assert!(!install.join(BACKUP_DIR_NAME).exists());
    }

    /// ポータブル版でない差し替え先や入れ子のフォルダを拒否すること
    #[test]
    fn install_portable_rejects_invalid_dirs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("new");
        portable(&source, "2");

        // マーカーの無いフォルダ (MSI 版など) へは差し替えない
        let msi = tmp.path().join("msi");
        write(&msi.join(APP_EXE_NAME), "app 1");
        assert_eq!(
            install_portable(&source, &msi),
            Err(UpdateError::InstallDirInvalid)
        );
        // 差し替え先の中にある配布物は使わない
        let install = tmp.path().join("app");
        portable(&install, "1");
        let nested = install.join("docs").join("new");
        portable(&nested, "2");
        assert_eq!(
            install_portable(&nested, &install),
            Err(UpdateError::InstallDirInvalid)
        );
        // 本体の無い配布物は使わない
        let broken = tmp.path().join("broken");
        write(&broken.join(PORTABLE_MARKER_FILE), "");
        assert_eq!(
            install_portable(&broken, &install),
            Err(UpdateError::PortableInvalid)
        );
    }
}
