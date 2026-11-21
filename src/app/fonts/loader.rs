//! フォントの読み込みを提供

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use eframe::egui::{FontData, FontTweak};
use known_folders::{KnownFolder, get_known_folder_path};

// ======================================================================
// システムフォント候補
// ======================================================================
/// システムフォントの候補
struct SystemFont {
    /// フォントフォルダ内のファイル名
    file_name: &'static str,
    /// `.ttc` 内のフォント番号
    index: u32,
    /// グリフを下へずらす量 (フォントサイズに対する比率)
    y_offset_factor: f32,
}

/// Windows の日本語フォント候補 (優先順位順)
///
/// 可読性の高い順 (Yu Gothic → Meiryo → MS ゴシック) に並べる
/// 通常版は行間 (line gap) が大きく、egui 標準フォントとベースラインがずれて
/// 絵文字や欧文との並びが崩れるため、行の高さが詰まった UI 版を使う
const SYSTEM_JP_FONTS: &[SystemFont] = &[
    // Yu Gothic UI Regular
    SystemFont {
        file_name: "YuGothM.ttc",
        index: 1,
        y_offset_factor: 0.0,
    },
    // Meiryo UI
    SystemFont {
        file_name: "meiryo.ttc",
        index: 2,
        y_offset_factor: 0.0,
    },
    // MS UI Gothic
    SystemFont {
        file_name: "msgothic.ttc",
        index: 1,
        y_offset_factor: 0.0,
    },
];

/// Windows のカラー絵文字フォント候補 (優先順位順)
///
/// egui の自動探索に任せると、絵文字が日本語の文字より 2px ほど上に描かれる
/// (行の高さが標準フォントより低く、ベースラインが上がるため)
/// 絵文字の中心を日本語の文字の中心へ合わせるよう、実測した縦メトリクスから求めた量だけ下げる
const SYSTEM_EMOJI_FONTS: &[SystemFont] = &[
    // Segoe UI Emoji
    SystemFont {
        file_name: "seguiemj.ttf",
        index: 0,
        y_offset_factor: 0.15,
    },
];

// ======================================================================
// フォントフォルダの解決
// ======================================================================
/// システムフォントフォルダを返す
///
/// インストール先の変更を反映するため `FOLDERID_Fonts` を優先し、
/// 取得できない場合は `%SystemRoot%\Fonts` とする
///
/// # Returns
/// * `Option<PathBuf>` - フォントフォルダ。どちらも決められない場合は `None`
fn system_fonts_dir() -> Option<PathBuf> {
    get_known_folder_path(KnownFolder::Fonts)
        .or_else(|| env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("Fonts")))
}

// ======================================================================
// フォント読み込み
// ======================================================================
/// 読み込んだシステムフォント
#[derive(Default)]
pub struct SystemFonts {
    /// 日本語フォント。未発見の場合 `None`
    pub japanese: Option<FontData>,
    /// カラー絵文字フォント。未発見の場合 `None`
    pub emoji: Option<FontData>,
}

/// 日本語・絵文字のシステムフォントを読み込む
///
/// Windows のシステムフォントフォルダから検索する
/// 見つからないフォントは `None` になる
///
/// # Returns
/// * `SystemFonts` - 読み込んだフォント
pub fn load_system_fonts() -> SystemFonts {
    let Some(dir) = system_fonts_dir() else {
        return SystemFonts::default();
    };
    SystemFonts {
        japanese: load_first_font(&dir, SYSTEM_JP_FONTS),
        emoji: load_first_font(&dir, SYSTEM_EMOJI_FONTS),
    }
}

/// フォルダ内の候補ファイルを順に試し、最初に読めたものを返す
///
/// # Arguments
/// * `dir` - 検索するフォルダ
/// * `candidates` - フォントの候補 (優先順位順)
///
/// # Returns
/// * `Option<FontData>` - 最初に読めたフォント (縦位置の補正込み)。1 つも読めない場合は `None`
fn load_first_font(dir: &Path, candidates: &[SystemFont]) -> Option<FontData> {
    candidates.iter().find_map(|c| {
        let data = fs::read(dir.join(c.file_name)).ok()?;
        let mut font = FontData::from_owned(data).tweak(FontTweak {
            y_offset_factor: c.y_offset_factor,
            ..FontTweak::default()
        });
        font.index = c.index;
        Some(font)
    })
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用の候補を作る
    fn font(file_name: &'static str, index: u32, y_offset_factor: f32) -> SystemFont {
        SystemFont {
            file_name,
            index,
            y_offset_factor,
        }
    }

    /// `load_system_fonts()` がパニックせずに呼び出せること
    #[test]
    fn load_system_fonts_does_not_panic() {
        let _ = load_system_fonts();
    }

    /// 日本語フォントが存在する環境では `Some` を返すこと
    #[test]
    fn load_system_fonts_returns_japanese_when_system_fonts_available() {
        let result = load_system_fonts();
        let any_path_exists = system_fonts_dir().is_some_and(|dir| {
            SYSTEM_JP_FONTS
                .iter()
                .any(|f| dir.join(f.file_name).exists())
        });
        assert_eq!(result.japanese.is_some(), any_path_exists);
    }

    /// 複数の候補がある場合は先頭に近いものを優先すること
    #[test]
    fn load_first_font_prefers_earlier_candidate() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("first.ttc"), b"first").unwrap();
        fs::write(dir.path().join("second.ttc"), b"second").unwrap();

        let font = load_first_font(
            dir.path(),
            &[font("first.ttc", 0, 0.0), font("second.ttc", 0, 0.0)],
        )
        .unwrap();
        assert_eq!(font.bytes(), b"first");
    }

    /// 先頭の候補が無い場合は次の候補へ代替すること
    #[test]
    fn load_first_font_falls_back_to_next_candidate() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("second.ttc"), b"second").unwrap();

        let font = load_first_font(
            dir.path(),
            &[font("missing.ttc", 0, 0.0), font("second.ttc", 0, 0.0)],
        )
        .unwrap();
        assert_eq!(font.bytes(), b"second");
    }

    /// 候補が 1 つも無い場合は `None` を返すこと
    #[test]
    fn load_first_font_returns_none_when_no_candidate_exists() {
        let dir = tempfile::tempdir().unwrap();

        assert!(load_first_font(dir.path(), &[font("missing.ttc", 0, 0.0)]).is_none());
    }

    /// 候補のフォント番号と縦位置の補正値がフォントデータへ設定されること
    #[test]
    fn load_first_font_applies_index_and_y_offset() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.ttc"), b"a").unwrap();

        let font = load_first_font(dir.path(), &[font("a.ttc", 2, 0.15)]).unwrap();
        assert_eq!(font.index, 2);
        assert!((font.tweak.y_offset_factor - 0.15).abs() < f32::EPSILON);
    }

    /// 日本語フォントの候補順が Yu Gothic → Meiryo → MS ゴシックであること
    #[test]
    fn japanese_candidates_are_ordered_by_readability() {
        let names: Vec<_> = SYSTEM_JP_FONTS.iter().map(|f| f.file_name).collect();
        assert_eq!(names, ["YuGothM.ttc", "meiryo.ttc", "msgothic.ttc"]);
    }
}
