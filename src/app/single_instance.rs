//! 多重起動の防止
//!
//! 利用者ごとに 1 つの排他ロックファイルを開いたままにして、同時に動く本体を 1 つに限る。
//! 設定の保存・更新プログラムによる終了依頼・更新用の作業フォルダの掃除は、いずれも
//! 本体が 1 つであることを前提にしている。ロックはプロセスの終了 (異常終了を含む) で OS が解放する

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::PathBuf;

/// ロックファイル名 (`%TEMP%` に置く)
const LOCK_FILE_NAME: &str = "EdgeCut.instance.lock";

/// 起動中の本体が持つロック (破棄すると解放する)
#[derive(Debug)]
pub(super) struct InstanceGuard {
    /// 排他ロックを保持するファイル (閉じるとロックも解ける)
    _file: File,
}

/// ロックファイルのパスを返す
///
/// # Returns
/// * `PathBuf` - `%TEMP%` 直下のロックファイル
fn lock_path() -> PathBuf {
    std::env::temp_dir().join(LOCK_FILE_NAME)
}

/// 本体の起動権を取得する
///
/// # Returns
/// * `io::Result<Option<InstanceGuard>>` - 取得できたら `Some`、別のウィンドウが起動中なら `None`
///
/// # Errors
/// ロックファイルを開けない場合、またはロックの取得で想定外のエラーになった場合
pub(super) fn acquire() -> io::Result<Option<InstanceGuard>> {
    acquire_at(&lock_path())
}

/// 指定したロックファイルで起動権を取得する
///
/// # Arguments
/// * `path` - ロックファイルのパス
///
/// # Returns
/// * `io::Result<Option<InstanceGuard>>` - 取得できたら `Some`、既に誰かが持っていれば `None`
///
/// # Errors
/// ファイルを開けない場合、またはロックの取得で想定外のエラーになった場合
fn acquire_at(path: &std::path::Path) -> io::Result<Option<InstanceGuard>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(InstanceGuard { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(err)) => Err(err),
    }
}

// ======================================================================
// 起動済みのウィンドウを前面に出す
// ======================================================================
/// 起動済みの本体のウィンドウを探して前面に出す (最小化していれば元に戻す)
///
/// # Returns
/// * `bool` - ウィンドウが見つかって前面に出せたら `true` (起動直後でまだ作られていないなどは `false`)
pub(super) fn activate_existing_window() -> bool {
    win32::activate_window_titled(crate::consts::APP_NAME)
}

/// ウィンドウの前面化に使う Win32 API
///
/// 追加の依存を増やさないよう `user32` の関数を直接宣言する。`unsafe` はこのモジュールに限る
#[expect(
    unsafe_code,
    reason = "他プロセスのウィンドウを前面に出すには Win32 API (user32) の直接呼び出しが必要で、安全なラッパーが無い"
)]
mod win32 {
    use std::os::windows::ffi::OsStrExt;

    /// `ShowWindow` の `SW_RESTORE` (最小化・最大化を元の大きさに戻す)
    const SW_RESTORE: i32 = 9;

    #[link(name = "user32")]
    unsafe extern "system" {
        /// クラス名・タイトルが一致する最上位ウィンドウを探す (見つからなければ 0)
        fn FindWindowW(class_name: *const u16, window_name: *const u16) -> isize;
        /// ウィンドウが最小化されているか
        fn IsIconic(hwnd: isize) -> i32;
        /// ウィンドウの表示状態を変える
        fn ShowWindow(hwnd: isize, cmd_show: i32) -> i32;
        /// ウィンドウを前面に出す
        fn SetForegroundWindow(hwnd: isize) -> i32;
    }

    /// タイトルが完全一致する最上位ウィンドウを前面に出す
    ///
    /// # Arguments
    /// * `title` - ウィンドウのタイトル
    ///
    /// # Returns
    /// * `bool` - ウィンドウが見つかったら `true`
    pub(super) fn activate_window_titled(title: &str) -> bool {
        let wide: Vec<u16> = std::ffi::OsStr::new(title)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: `wide` は NUL 終端の UTF-16 文字列で、呼び出しの間は有効。
        // クラス名に null を渡すのは API が認めている (タイトルだけで探す)
        let hwnd = unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) };
        if hwnd == 0 {
            return false;
        }
        // SAFETY: `hwnd` は直前に取得したウィンドウハンドル。無効になっていても
        // これらの API は失敗を返すだけで、メモリ安全性には影響しない
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
        }
        true
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// 保持中は 2 つ目を取得できず、解放後は取得できること
    #[test]
    fn second_acquire_is_refused_until_released() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("test.lock");

        let first = acquire_at(&path).expect("first").expect("guard");
        assert!(acquire_at(&path).expect("second").is_none());

        drop(first);
        assert!(acquire_at(&path).expect("third").is_some());
    }
}
