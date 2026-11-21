//! 統合テスト共通モジュール

#[expect(
    dead_code,
    reason = "複数のテストクレートで共有され、使わないヘルパーがある"
)]
pub mod images;
