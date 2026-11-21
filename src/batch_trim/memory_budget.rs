//! 並列処理で共有するメモリ予算

use std::sync::{Condvar, Mutex, PoisonError};

// ======================================================================
// メモリ予算
// ======================================================================
/// 並列処理するファイル間で共有するメモリ予算
///
/// 確保できるまで待つ計数セマフォとして働く。各ワーカーが保持する許可は高々 1 つのため、
/// 1 件の要求を予算全体に切り詰めておけばデッドロックしない
pub(super) struct MemoryBudget {
    /// 予算全体のバイト数
    capacity: u64,
    /// 未確保のバイト数
    available: Mutex<u64>,
    /// 許可の返却を待つワーカーへの通知
    released: Condvar,
}

impl MemoryBudget {
    /// 予算全体が未確保の状態で生成する
    pub(super) fn new(capacity: u64) -> Self {
        Self {
            capacity,
            available: Mutex::new(capacity),
            released: Condvar::new(),
        }
    }

    /// `bytes` を確保できるまで待ち、確保した許可を返す
    ///
    /// 予算全体を超える要求は予算全体に切り詰め、他の許可がすべて返却された時点で確保する
    ///
    /// # Arguments
    /// * `bytes` - 確保するバイト数
    ///
    /// # Returns
    /// * `MemoryPermit` - 破棄時 (panic による巻き戻しを含む) に確保分を返却する許可
    pub(super) fn acquire(&self, bytes: u64) -> MemoryPermit<'_> {
        let bytes = bytes.min(self.capacity);
        self.wait_and_take(bytes);
        MemoryPermit {
            budget: self,
            bytes,
        }
    }

    /// `bytes` (予算全体以下) を確保できるまで待ち、未確保のバイト数から差し引く
    fn wait_and_take(&self, bytes: u64) {
        // 保護対象は数値 1 つで、panic 後も不整合にならない
        let mut available = self
            .released
            .wait_while(
                self.available
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner),
                |available| *available < bytes,
            )
            .unwrap_or_else(PoisonError::into_inner);
        *available -= bytes;
    }
}

/// `MemoryBudget` から確保したメモリの許可
pub(super) struct MemoryPermit<'a> {
    /// 返却先の予算
    budget: &'a MemoryBudget,
    /// 確保したバイト数
    bytes: u64,
}

impl MemoryPermit<'_> {
    /// 確保量を `bytes` まで増やす
    ///
    /// 現在の確保量以下の場合は何もしない。予算全体を超える要求は予算全体に切り詰める
    /// 差額を直ちに確保できれば保持したまま増やす
    /// 確保できない場合は、保持分をいったん返却してから `bytes` 全体を確保し直す
    /// (許可を保持したまま待つと、複数のワーカーが互いに待ち合ってデッドロックするため)
    ///
    /// # Arguments
    /// * `bytes` - 増やした後の確保量
    pub(super) fn grow_to(&mut self, bytes: u64) {
        let budget = self.budget;
        let bytes = bytes.min(budget.capacity);
        if bytes <= self.bytes {
            return;
        }
        {
            let mut available = budget
                .available
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let extra = bytes - self.bytes;
            if *available >= extra {
                *available -= extra;
                self.bytes = bytes;
                return;
            }
            *available += self.bytes;
            self.bytes = 0;
        }
        budget.released.notify_all();
        budget.wait_and_take(bytes);
        self.bytes = bytes;
    }
}

impl Drop for MemoryPermit<'_> {
    fn drop(&mut self) {
        *self
            .budget
            .available
            .lock()
            .unwrap_or_else(PoisonError::into_inner) += self.bytes;
        self.budget.released.notify_all();
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    use super::*;
    use crate::panic_message::catch_panic;

    /// 同時に保持される許可の合計が予算を超えないこと
    #[test]
    fn memory_budget_limits_concurrent_permits() {
        let budget = MemoryBudget::new(100);
        let active = AtomicUsize::new(0);
        let max_active = AtomicUsize::new(0);

        thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    for _ in 0..5 {
                        let _permit = budget.acquire(60);
                        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                        max_active.fetch_max(now, Ordering::SeqCst);
                        // 他スレッドと重なる機会を作る
                        thread::sleep(std::time::Duration::from_millis(1));
                        active.fetch_sub(1, Ordering::SeqCst);
                    }
                });
            }
        });

        // 60 バイトの許可は 100 バイトの予算に 1 つしか入らない
        assert_eq!(max_active.into_inner(), 1);
        assert_eq!(*budget.available.lock().unwrap(), 100);
    }

    /// 予算全体を超える要求も予算全体に切り詰めて確保できること
    #[test]
    fn memory_budget_clamps_oversized_request() {
        let budget = MemoryBudget::new(100);
        let permit = budget.acquire(u64::MAX);

        assert_eq!(permit.bytes, 100);
        assert_eq!(*budget.available.lock().unwrap(), 0);
        // 破棄すると全額が返却される
        drop(permit);
        assert_eq!(*budget.available.lock().unwrap(), 100);
    }

    /// 空きがあれば保持したまま差額だけ追加で確保し、確保量を減らす要求は無視すること
    #[test]
    fn memory_permit_grow_takes_only_the_difference() {
        let budget = MemoryBudget::new(100);
        let mut permit = budget.acquire(30);

        permit.grow_to(50);
        assert_eq!(permit.bytes, 50);
        assert_eq!(*budget.available.lock().unwrap(), 50);

        // 小さい値への変更は確保量を変えない
        permit.grow_to(10);
        assert_eq!(permit.bytes, 50);

        // 予算全体を超える要求は予算全体に切り詰める
        permit.grow_to(u64::MAX);
        assert_eq!(permit.bytes, 100);
        assert_eq!(*budget.available.lock().unwrap(), 0);

        drop(permit);
        assert_eq!(*budget.available.lock().unwrap(), 100);
    }

    /// 空きが足りない状態で増やそうとする複数のワーカーがデッドロックしないこと
    #[test]
    fn memory_permit_grow_does_not_deadlock() {
        let budget = MemoryBudget::new(100);

        thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    for _ in 0..5 {
                        // 4 つの許可で予算の全体を使い切り、全員が増やす側に回る
                        let mut permit = budget.acquire(25);
                        thread::sleep(std::time::Duration::from_millis(1));
                        permit.grow_to(60);
                        assert_eq!(permit.bytes, 60);
                    }
                });
            }
        });

        assert_eq!(*budget.available.lock().unwrap(), 100);
    }

    /// 許可を保持したまま panic しても確保分が返却されること
    #[test]
    fn memory_budget_releases_permit_on_panic() {
        let budget = MemoryBudget::new(100);
        let result = catch_panic(|| {
            let _permit = budget.acquire(80);
            panic!("テスト用の panic");
        });

        assert!(result.is_err());
        assert_eq!(*budget.available.lock().unwrap(), 100);
    }
}
