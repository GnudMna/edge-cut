//! フォント設定を提供

use super::loader;

use eframe::egui::{
    Context, FontData, FontFamily,
    epaint::text::{FontInsert, FontPriority, InsertFontFamily},
};

// ======================================================================
// フォント登録
// ======================================================================
/// 日本語フォントの登録名
pub const JAPANESE_FONT_KEY: &str = "japanese_font";

/// 日本語フォントをフォントへ追加する
///
/// OS のシステムフォントから日本語フォントを検索して登録する
/// 見つからない場合はフォントを追加しない
///
/// # Arguments
/// * `ctx` - 追加対象のGUIコンテキスト
pub fn regist_japanese_font(ctx: &Context) {
    if let Some(font_data) = loader::load_japanese_font() {
        regist_font(ctx, JAPANESE_FONT_KEY, font_data);
    }
}

/// フォントを追加
///
/// 既存のフォント設定を保持したまま新しいフォントを追加する
///
/// # Arguments
/// * `ctx` - 追加対象の GUI コンテキスト
/// * `font_name` - 追加するフォントの名前
/// * `font_data` - 追加するフォントのデータ
fn regist_font(ctx: &Context, font_name: &str, font_data: FontData) {
    ctx.add_font(FontInsert::new(
        font_name,
        font_data,
        vec![
            // フォントファミリーへ追加
            InsertFontFamily {
                family: FontFamily::Proportional,
                priority: FontPriority::Highest,
            },
            // モノスペースファミリーへ追加
            InsertFontFamily {
                family: FontFamily::Monospace,
                priority: FontPriority::Lowest,
            },
        ],
    ));
}
