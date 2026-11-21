//! 一括トリミングのコア処理
//!
//! 設定・集計結果 (`result`)、エラー型 (`error`)、実行 (`runner`)、
//! 並列処理のメモリ予算 (`memory_budget`) に分け、ここでは公開 API を再エクスポートする。

mod error;
mod memory_budget;
mod result;
mod runner;

pub use self::error::BatchFileError;
pub use self::result::{BatchProgress, BatchTrimParams, BatchTrimResult};
pub use self::runner::run_batch_trim;

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests;
