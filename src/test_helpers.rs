// テスト用の画像生成ヘルパー (`tests/common/images.rs` を共有)

#![expect(
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    reason = "テストヘルパーのため、panic の説明と #[must_use] は不要"
)]

include!("../tests/common/images.rs");
