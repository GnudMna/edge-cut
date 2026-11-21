//! フォント関連のモジュール
mod loader;
mod registry;

pub use loader::load_system_fonts;
pub use registry::register_system_fonts;
