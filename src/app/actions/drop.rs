//! ドラッグ & ドロップされたファイル・フォルダの処理

use std::fs;
use std::path::PathBuf;

use super::batch::skipped_note;
use crate::app::state::App;
use crate::consts::is_supported_image_extension;

use eframe::egui;

// ======================================================================
// ドラッグ & ドロップ
// ======================================================================
impl App {
    /// ドロップされたファイル・フォルダを件数に応じて処理する
    ///
    /// フォルダは直下のファイルに展開する (サブフォルダは走査しない)。対応外の拡張子は除外する。
    /// ファイルのみで対象が 1 件ならプレビューで開き、2 件以上またはフォルダを含む場合は
    /// 保存先フォルダを選択させて一括トリミングする。
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト
    /// * `paths` - ドロップされたパス
    pub(in crate::app) fn handle_dropped_paths(
        &mut self,
        ctx: &egui::Context,
        paths: Vec<PathBuf>,
    ) {
        if paths.is_empty() {
            return;
        }
        let expanded = expand_dropped_dirs(paths);
        let (images, skipped) = partition_supported_images(expanded.paths);
        let skipped = skipped + expanded.unreadable_dirs;
        match images.as_slice() {
            [] if expanded.has_dir => {
                let note = skipped_note(skipped);
                self.status = format!("フォルダに対応画像がありません{note}");
            }
            [] => {
                self.status = format!("対応していないファイルです ({skipped} 件)");
            }
            [path] if !expanded.has_dir => {
                self.load_image_from_path(ctx, path);
                if skipped > 0 {
                    let note = skipped_note(skipped);
                    self.status.push_str(&note);
                    // 読み込み完了でステータスが上書きされても除外件数を残す
                    self.io.load_note = Some(note);
                }
            }
            _ if self.batch.running => {
                self.status =
                    "一括トリミングの実行中は複数ファイル・フォルダのドロップを受け付けません"
                        .into();
            }
            _ => self.start_batch_trim(ctx, images, skipped),
        }
    }
}

/// フォルダを展開したドロップ対象
struct ExpandedDrop {
    /// ファイルのパスと、フォルダ直下のエントリ (フォルダごとに名前順)
    paths: Vec<PathBuf>,
    /// フォルダを 1 つ以上含んでいたか
    has_dir: bool,
    /// 読み取りに失敗したフォルダの件数
    unreadable_dirs: usize,
}

/// ドロップされたパスのうちフォルダを直下のエントリに展開する
///
/// サブフォルダは再帰せずそのままエントリとして残す (後段の振り分けで除外される)
/// `read_dir` の順序は OS 依存のため、フォルダごとに名前順へ並べ替える
///
/// # Arguments
/// * `paths` - ドロップされたパス
///
/// # Returns
/// * `ExpandedDrop` - 展開後のパス (入力順を維持)、フォルダの有無、読み取り失敗件数
fn expand_dropped_dirs(paths: Vec<PathBuf>) -> ExpandedDrop {
    let mut expanded = ExpandedDrop {
        paths: Vec::with_capacity(paths.len()),
        has_dir: false,
        unreadable_dirs: 0,
    };
    for path in paths {
        if !path.is_dir() {
            expanded.paths.push(path);
            continue;
        }
        expanded.has_dir = true;
        let Ok(entries) = fs::read_dir(&path) else {
            expanded.unreadable_dirs += 1;
            continue;
        };
        let mut children: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        children.sort();
        expanded.paths.extend(children);
    }
    expanded
}

/// パスを対応画像ファイルとそれ以外に振り分ける
///
/// 拡張子の大文字・小文字は区別しない。フォルダは拡張子に関係なく対象外とする
///
/// # Arguments
/// * `paths` - 振り分けるパス
///
/// # Returns
/// * `(Vec<PathBuf>, usize)` - 対応画像ファイル (入力順を維持) と除外した件数
fn partition_supported_images(paths: Vec<PathBuf>) -> (Vec<PathBuf>, usize) {
    let total = paths.len();
    let images: Vec<PathBuf> = paths
        .into_iter()
        .filter(|p| {
            !p.is_dir()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| is_supported_image_extension(&e.to_ascii_lowercase()))
        })
        .collect();
    let skipped = total - images.len();
    (images, skipped)
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{expand_dropped_dirs, partition_supported_images};
    use crate::app::actions::test_helpers::{make_test_image, poll_io_until};
    use crate::app::state::App;

    use eframe::egui;

    // ======================================================================
    // handle_dropped_paths() / partition_supported_images()
    // ======================================================================
    /// 対応拡張子のファイルのみを入力順のまま残し、除外件数を返すことを確認する
    #[test]
    fn partition_supported_images_filters_by_extension() {
        let paths = vec![
            PathBuf::from("b.PNG"),
            PathBuf::from("note.txt"),
            PathBuf::from("a.jpeg"),
            PathBuf::from("no_ext"),
        ];
        let (images, skipped) = partition_supported_images(paths);
        // 大文字の拡張子も対応形式として扱う
        assert_eq!(
            images,
            vec![PathBuf::from("b.PNG"), PathBuf::from("a.jpeg")]
        );
        assert_eq!(skipped, 2);
    }

    /// 画像の拡張子を持つフォルダを除外することを確認する
    #[test]
    fn partition_supported_images_skips_directories() {
        let tmp = tempfile::Builder::new().suffix(".png").tempdir().unwrap();
        let (images, skipped) = partition_supported_images(vec![tmp.path().to_path_buf()]);

        assert!(images.is_empty());
        assert_eq!(skipped, 1);
    }

    /// 対応ファイルがない場合に件数をステータスへ表示し、読み込みを行わないことを確認する
    #[test]
    fn handle_dropped_paths_reports_unsupported_only() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.handle_dropped_paths(&ctx, vec![PathBuf::from("a.txt"), PathBuf::from("b.pdf")]);

        assert!(
            app.status.contains("対応していない") && app.status.contains("2 件"),
            "status = {}",
            app.status
        );
        assert!(app.image.original.is_none());
    }

    /// 空のドロップではステータスを変更しないことを確認する
    #[test]
    fn handle_dropped_paths_ignores_empty() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let status_before = app.status.clone();
        app.handle_dropped_paths(&ctx, Vec::new());
        assert_eq!(app.status, status_before);
    }

    /// 対応画像が 1 件ならプレビューで開き、完了後のステータスにも除外件数を残すことを確認する
    #[test]
    fn handle_dropped_paths_loads_single_image_with_note() {
        let ctx = egui::Context::default();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("input.png");
        make_test_image().save(&path).unwrap();

        let mut app = App::default();
        app.handle_dropped_paths(&ctx, vec![tmp.path().join("readme.txt"), path]);
        // 読み込み中の時点で除外件数を表示する
        assert!(app.status.contains("読み込み中"), "status = {}", app.status);
        assert!(app.status.contains("除外"), "status = {}", app.status);

        let done = poll_io_until(&mut app, &ctx, |a| a.image.original.is_some());

        // 読み込み完了で上書きされても補足は残る
        assert!(done, "元画像が設定されなかった: {}", app.status);
        assert!(
            app.status.contains("読み込み完了") && app.status.contains("対応外の 1 件"),
            "status = {}",
            app.status
        );
    }

    /// 一括トリミングの実行中は複数ファイルのドロップを受け付けないことを確認する
    #[test]
    fn handle_dropped_paths_rejects_batch_while_running() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.batch.running = true;
        app.handle_dropped_paths(&ctx, vec![PathBuf::from("a.png"), PathBuf::from("b.png")]);
        assert!(
            app.status.contains("受け付けません"),
            "status = {}",
            app.status
        );
    }

    // ======================================================================
    // expand_dropped_dirs() / フォルダのドロップ
    // ======================================================================
    /// フォルダを直下のエントリへ名前順に展開し、ファイルは入力順のまま残すことを確認する
    #[test]
    fn expand_dropped_dirs_lists_direct_children_sorted() {
        let ws = tempfile::tempdir().unwrap();
        let tmp = ws.path();
        let sub = tmp.join("sub");
        fs::create_dir_all(&sub).unwrap();
        for name in ["b.png", "a.png"] {
            fs::write(tmp.join(name), b"").unwrap();
        }
        fs::write(sub.join("nested.png"), b"").unwrap();

        let expanded = expand_dropped_dirs(vec![PathBuf::from("x.png"), tmp.to_path_buf()]);

        // サブフォルダは中身を展開せずエントリとして残す
        assert_eq!(
            expanded.paths,
            vec![
                PathBuf::from("x.png"),
                tmp.join("a.png"),
                tmp.join("b.png"),
                sub,
            ]
        );
        assert!(expanded.has_dir);
        assert_eq!(expanded.unreadable_dirs, 0);
    }

    /// フォルダを含まない場合は入力をそのまま返すことを確認する
    #[test]
    fn expand_dropped_dirs_keeps_files_as_is() {
        let paths = vec![PathBuf::from("b.png"), PathBuf::from("a.txt")];
        let expanded = expand_dropped_dirs(paths.clone());
        assert_eq!(expanded.paths, paths);
        assert!(!expanded.has_dir);
    }

    /// 対応画像のないフォルダでは専用のメッセージと除外件数を表示することを確認する
    #[test]
    fn handle_dropped_paths_reports_folder_without_images() {
        let ctx = egui::Context::default();
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("sub")).unwrap();
        fs::write(tmp.path().join("readme.txt"), b"").unwrap();

        let mut app = App::default();
        app.handle_dropped_paths(&ctx, vec![tmp.path().to_path_buf()]);

        // サブフォルダとテキストファイルの 2 件を除外として数える
        assert!(
            app.status.contains("フォルダに対応画像がありません") && app.status.contains("2 件"),
            "status = {}",
            app.status
        );
        assert!(app.image.original.is_none());
    }

    /// 対応画像が 1 件だけのフォルダでもプレビューではなく一括トリミングとして扱うことを確認する
    #[test]
    fn handle_dropped_paths_treats_folder_as_batch() {
        let ctx = egui::Context::default();
        let tmp = tempfile::tempdir().unwrap();
        make_test_image()
            .save(tmp.path().join("input.png"))
            .unwrap();

        // 保存先ダイアログを開かずに一括トリミング経路へ入ったことを確認するため実行中にしておく
        let mut app = App::default();
        app.batch.running = true;
        app.handle_dropped_paths(&ctx, vec![tmp.path().to_path_buf()]);

        assert!(
            app.status.contains("受け付けません"),
            "status = {}",
            app.status
        );
        assert!(app.io.load_note.is_none());
    }
}
