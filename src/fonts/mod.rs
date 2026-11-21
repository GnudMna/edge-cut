//! フォント関連のモジュール
mod loader;
mod registry;

pub use registry::regist_japanese_font;

#[allow(unused)]
pub use registry::JAPANESE_FONT_KEY;
