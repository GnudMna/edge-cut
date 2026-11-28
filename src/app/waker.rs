//! ワーカースレッドから UI スレッドのイベントループを起こす通知
//!
//! ワーカーは結果をチャネルへ送った後に [`Waker::wake`] を呼び、イベントループに結果の取り込みと
//! 画面状態の送信を促す。テストではイベントループを持たない [`Waker::noop`] を使う

use std::fmt;
use std::sync::Arc;

/// イベントループを起こす通知
///
/// 複製して各ワーカースレッドへ渡す。イベントループの終了後に呼んでも何もしない
#[derive(Clone)]
pub(in crate::app) struct Waker {
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Waker {
    /// 通知のたびに `wake` を呼ぶ `Waker` を作る
    ///
    /// # Arguments
    /// * `wake` - イベントループを起こす処理 (任意のスレッドから呼ばれる)
    ///
    /// # Returns
    /// * `Self` - 生成した `Waker`
    pub(in crate::app) fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            wake: Some(Arc::new(wake)),
        }
    }

    /// 何もしない `Waker` を作る (イベントループを持たないテスト用)
    ///
    /// # Returns
    /// * `Self` - 通知しても何も起きない `Waker`
    pub(in crate::app) fn noop() -> Self {
        Self { wake: None }
    }

    /// イベントループを起こす
    pub(in crate::app) fn wake(&self) {
        if let Some(wake) = &self.wake {
            wake();
        }
    }
}

impl Default for Waker {
    fn default() -> Self {
        Self::noop()
    }
}

impl fmt::Debug for Waker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Waker")
            .field("connected", &self.wake.is_some())
            .finish()
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// 複製した `Waker` からも同じ処理が呼ばれること
    #[test]
    fn wake_calls_shared_callback() {
        let count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&count);
        let waker = Waker::new(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        });
        waker.wake();
        waker.clone().wake();
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }

    /// `noop` は呼んでも panic しないこと
    #[test]
    fn noop_does_nothing() {
        Waker::noop().wake();
    }
}
