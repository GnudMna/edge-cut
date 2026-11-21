//! 処理中の panic を捕捉し、メッセージ文字列として取り出す
//!
//! ワーカースレッドの panic を 1 件の失敗として UI に表示するために使う

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};

/// 処理を実行し、panic を `Err(panic メッセージ)` に変換する
///
/// 呼び出し側は、処理が共有する可変状態を持たない (または panic 後に破棄する) ことを保証する
///
/// # Arguments
/// * `run` - 実行する処理
///
/// # Returns
/// * `Result<T, String>` - 処理の戻り値。panic した場合は panic メッセージ
pub(crate) fn catch_panic<T>(run: impl FnOnce() -> T) -> Result<T, String> {
    panic::catch_unwind(AssertUnwindSafe(run)).map_err(|payload| panic_message(payload.as_ref()))
}

/// panic のペイロードから表示用のメッセージを取り出す
///
/// `panic!` の引数は `&str` または `String` として渡されるため、それ以外は固定文言にする
///
/// # Arguments
/// * `payload` - `catch_unwind` が返したペイロード
///
/// # Returns
/// * `String` - panic メッセージ。取り出せない場合は `"不明なエラー"`
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "不明なエラー".to_owned()
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// `&str`・`String`・その他のペイロードをそれぞれ文字列化できることを確認する
    #[test]
    fn extracts_message_from_payload() {
        // リテラルの panic は `&str`
        let payload = panic::catch_unwind(|| panic!("literal")).unwrap_err();
        assert_eq!(panic_message(payload.as_ref()), "literal");

        // 書式付きの panic は `String`
        let n = 1;
        let payload = panic::catch_unwind(|| panic!("formatted {n}")).unwrap_err();
        assert_eq!(panic_message(payload.as_ref()), "formatted 1");

        // それ以外の型は固定文言
        let payload = panic::catch_unwind(|| panic::panic_any(42_i32)).unwrap_err();
        assert_eq!(panic_message(payload.as_ref()), "不明なエラー");
    }

    /// 正常時は戻り値を、panic 時はメッセージを返すことを確認する
    #[test]
    fn catch_panic_converts_panic_to_error() {
        // 正常時は戻り値をそのまま返す
        assert_eq!(catch_panic(|| 42), Ok(42));

        // panic はメッセージ付きの Err に変換される
        let result: Result<(), String> = catch_panic(|| panic!("テスト用の panic"));
        assert_eq!(result, Err("テスト用の panic".to_owned()));
    }
}
