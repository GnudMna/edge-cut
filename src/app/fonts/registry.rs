//! フォント設定を提供

use eframe::egui::{
    Context, FontData, FontFamily,
    epaint::text::{FontInsert, FontPriority, InsertFontFamily},
};

use super::loader::SystemFonts;

// ======================================================================
// フォント登録
// ======================================================================
/// 日本語フォントの登録名
pub const JAPANESE_FONT_KEY: &str = "japanese_font";

/// 絵文字フォントの登録名
pub const EMOJI_FONT_KEY: &str = "emoji_font";

/// システムフォントをフォントへ追加する
///
/// 欧文・数字は egui 標準のフォントで描画し続けるよう、最低優先度の代替として追加する
/// 日本語フォントを先に追加するため、両方が持つ文字は日本語フォントで描画する
/// 見つからないフォントは追加しない
///
/// # Arguments
/// * `ctx` - 追加対象のGUIコンテキスト
/// * `fonts` - 追加するフォント
///
/// # Returns
/// * `bool` - 日本語フォントを追加した場合 `true`
pub fn register_system_fonts(ctx: &Context, fonts: SystemFonts) -> bool {
    let japanese_registered = fonts.japanese.is_some();
    if let Some(font_data) = fonts.japanese {
        register_font(ctx, JAPANESE_FONT_KEY, font_data);
    }
    if let Some(font_data) = fonts.emoji {
        register_font(ctx, EMOJI_FONT_KEY, font_data);
    }
    japanese_registered
}

/// フォントを追加
///
/// 既存のフォント設定を保持したまま新しいフォントを追加する
///
/// # Arguments
/// * `ctx` - 追加対象の GUI コンテキスト
/// * `font_name` - 追加するフォントの名前
/// * `font_data` - 追加するフォントのデータ
fn register_font(ctx: &Context, font_name: &str, font_data: FontData) {
    ctx.add_font(FontInsert::new(
        font_name,
        font_data,
        vec![
            // 標準フォントに無い文字 (日本語・絵文字) の代替として追加
            InsertFontFamily {
                family: FontFamily::Proportional,
                priority: FontPriority::Lowest,
            },
            // モノスペースファミリーへ追加
            InsertFontFamily {
                family: FontFamily::Monospace,
                priority: FontPriority::Lowest,
            },
        ],
    ));
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// 日本語フォントが無い場合は `false` を返すこと
    #[test]
    fn register_returns_false_without_japanese_font() {
        let fonts = SystemFonts {
            japanese: None,
            emoji: Some(FontData::from_owned(Vec::new())),
        };
        assert!(!register_system_fonts(&Context::default(), fonts));
    }

    /// 日本語フォントがある場合は `true` を返すこと
    #[test]
    fn register_returns_true_with_japanese_font() {
        let fonts = SystemFonts {
            japanese: Some(FontData::from_owned(Vec::new())),
            emoji: None,
        };
        assert!(register_system_fonts(&Context::default(), fonts));
    }
}
