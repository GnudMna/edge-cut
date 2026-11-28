//! 更新の進行 (別スレッド) と画面 (イベントループ) の橋渡し
//!
//! 進行のスレッドはページを書き込んでイベントループを起こし、利用者の操作は条件変数で待つ。
//! イベントループは UI のコマンドとウィンドウを閉じる操作を、操作の結果または中止の要求として書き戻す

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use crate::update::flow::{Action, Page, Ui};

// ======================================================================
// 共有状態
// ======================================================================
/// 進行と画面で共有する状態
#[derive(Debug)]
struct State {
    /// 表示中のページ
    page: Page,
    /// 利用者の操作を待っているか
    waiting: bool,
    /// 選ばれた操作 (進行が受け取るまで保持する)
    action: Option<Action>,
    /// ダウンロードの中止を求められたか
    cancel: bool,
}

/// 進行と画面の橋渡し
#[derive(Clone)]
pub(super) struct Bridge {
    inner: Arc<(Mutex<State>, Condvar)>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Bridge {
    /// 最初のページと、ページが変わったときにイベントループを起こす処理から作る
    ///
    /// # Arguments
    /// * `page` - 最初に表示するページ
    /// * `wake` - イベントループを起こす
    ///
    /// # Returns
    /// * `Self` - 橋渡し
    pub(super) fn new(page: Page, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let state = State {
            page,
            waiting: false,
            action: None,
            cancel: false,
        };
        Self {
            inner: Arc::new((Mutex::new(state), Condvar::new())),
            wake: Arc::new(wake),
        }
    }

    /// 共有状態をロックする
    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 表示中のページ
    ///
    /// # Arguments
    /// * `self` - 橋渡し
    ///
    /// # Returns
    /// * `Page` - 表示中のページ
    pub(super) fn page(&self) -> Page {
        self.lock().page.clone()
    }

    /// 利用者の操作を進行へ渡す (操作を待っていなければ無視する)
    ///
    /// # Arguments
    /// * `self` - 橋渡し
    /// * `action` - 選ばれた操作
    pub(super) fn choose(&self, action: Action) {
        let mut state = self.lock();
        if state.waiting {
            state.waiting = false;
            state.action = Some(action);
            self.inner.1.notify_all();
        }
    }

    /// ウィンドウを閉じる操作を処理する
    ///
    /// 操作を待っていれば「閉じる」を選んだことにし、ダウンロード中なら中止を求める
    ///
    /// # Arguments
    /// * `self` - 橋渡し
    ///
    /// # Returns
    /// * `bool` - 進行を待たずにウィンドウを閉じてよい場合は `true` (更新を確認中)
    pub(super) fn request_close(&self) -> bool {
        let mut state = self.lock();
        if state.waiting {
            state.waiting = false;
            state.action = Some(Action::Close);
            self.inner.1.notify_all();
            return false;
        }
        if state.page.is_cancelable() {
            state.cancel = true;
            return false;
        }
        matches!(state.page, Page::Checking)
    }

    /// ダウンロードの中止を求める (ダウンロード中でなければ無視する)
    ///
    /// # Arguments
    /// * `self` - 橋渡し
    pub(super) fn cancel(&self) {
        let mut state = self.lock();
        if state.page.is_cancelable() {
            state.cancel = true;
        }
    }

    /// ページを差し替えてイベントループを起こす
    fn set_page(&self, page: Page, waiting: bool) {
        {
            let mut state = self.lock();
            state.page = page;
            state.waiting = waiting;
            state.action = None;
        }
        (self.wake)();
    }
}

impl Ui for Bridge {
    fn show(&self, page: Page) {
        self.set_page(page, false);
    }

    fn ask(&self, page: Page) -> Action {
        self.set_page(page, true);
        let (lock, cond) = &*self.inner;
        let mut state = lock.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(action) = state.action.take() {
                return action;
            }
            state = cond.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn cancel_requested(&self) -> bool {
        self.lock().cancel
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    use super::*;

    /// ダウンロード中のページ
    fn downloading() -> Page {
        Page::Downloading {
            latest: "1.1.0".into(),
            received: 0,
            total: 10,
        }
    }

    /// 操作を待つ進行へ、選ばれた操作を渡すこと
    #[test]
    fn ask_returns_chosen_action() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&wakes);
        let bridge = Bridge::new(Page::Checking, move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let flow = bridge.clone();
        let handle = thread::spawn(move || flow.ask(Page::Canceled));

        // 進行が待ち始めるまで選んでも無視されるため、待ち始めてから選ぶ
        while !bridge.lock().waiting {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(bridge.page(), Page::Canceled);
        bridge.choose(Action::LaunchApp);

        assert_eq!(handle.join().expect("join"), Action::LaunchApp);
        assert_eq!(wakes.load(Ordering::SeqCst), 1);
    }

    /// 閉じる操作は、待機中なら「閉じる」、ダウンロード中なら中止として扱うこと
    #[test]
    fn request_close_depends_on_page() {
        let bridge = Bridge::new(Page::Checking, || {});
        // 確認中は進行を待たずに閉じてよい
        assert!(bridge.request_close());

        bridge.show(downloading());
        assert!(!bridge.request_close());
        assert!(bridge.cancel_requested());

        // インストール中は閉じない
        let bridge = Bridge::new(Page::WaitingExit, || {});
        assert!(!bridge.request_close());
        assert!(!bridge.cancel_requested());
    }

    /// ダウンロード中以外の中止の要求は無視すること
    #[test]
    fn cancel_only_while_downloading() {
        let bridge = Bridge::new(Page::Starting, || {});
        bridge.cancel();
        assert!(!bridge.cancel_requested());

        bridge.show(downloading());
        bridge.cancel();
        assert!(bridge.cancel_requested());
    }
}
