//! ドラッグ & ドロップされたファイル・フォルダの処理

use std::fs;
use std::path::PathBuf;
use std::thread;

use eframe::egui;

use super::batch::skipped_note;
use crate::app::state::{App, IoMsg, ScannedDrop};
use crate::consts::is_supported_image_extension;
use crate::panic_message::catch_panic;

// ======================================================================
// ドラッグ & ドロップ
// ======================================================================
impl App {
    /// ドロップされたファイル・フォルダの展開をワーカースレッドで開始する
    ///
    /// `read_dir` や `is_dir` はネットワークドライブや大量のファイルで長く待つ場合があるため、
    /// UI スレッドでは行わない。結果は `IoMsg::DropScanned` で通知され、
    /// `poll_io` から `apply_scanned_drop` で処理する。新しいドロップが始まると古い結果は無視する
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト (完了時の再描画に使用)
    /// * `paths` - ドロップされたパス
    pub(in crate::app) fn handle_dropped_paths(
        &mut self,
        ctx: &egui::Context,
        paths: Vec<PathBuf>,
    ) {
        if paths.is_empty() {
            return;
        }
        self.io.drop_request_id = self.io.drop_request_id.wrapping_add(1);
        let request_id = self.io.drop_request_id;
        self.status = "ドロップされたファイルを確認中...".into();

        let tx = self.io.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = catch_panic(|| scan_dropped_paths(paths))
                .map_err(|msg| format!("ドロップの処理中にエラーが発生しました: {msg}"));
            let _ = tx.send(IoMsg::DropScanned { request_id, result });
            ctx.request_repaint();
        });
    }

    /// 展開済みのドロップ対象を件数に応じて処理する
    ///
    /// ファイルのみで対象が 1 件ならプレビューで開き、2 件以上またはフォルダを含む場合は
    /// 保存先フォルダを選択させて一括トリミングする。
    ///
    /// # Arguments
    /// * `ctx` - egui のコンテキスト
    /// * `scanned` - 展開・振り分け済みのドロップ対象
    pub(in crate::app) fn apply_scanned_drop(&mut self, ctx: &egui::Context, scanned: ScannedDrop) {
        let ScannedDrop {
            images,
            skipped,
            has_dir,
        } = scanned;
        match images.as_slice() {
            [] if has_dir => {
                let note = skipped_note(skipped);
                self.status = format!("フォルダに対応画像がありません{note}");
            }
            [] => {
                self.status = format!("対応していないファイルです ({skipped} 件)");
            }
            [path] if !has_dir => {
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
                    "一括トリミングの実行中は複数ファイル・フォルダのドロップ・貼り付けを受け付けません"
                        .into();
            }
            _ if self.io.dialog_open => {
                // 保存先ダイアログを開けないため、ドロップを処理しなかったことを明示する
                self.status = format!(
                    "ファイルダイアログを表示中のため、ドロップ・貼り付けした {} 件を処理できませんでした。ダイアログを閉じてからもう一度操作してください",
                    images.len()
                );
            }
            _ => self.start_batch_trim(ctx, images, skipped),
        }
    }
}

// ======================================================================
// 展開・振り分け (ワーカースレッド)
// ======================================================================
/// ドロップされたパスを展開し、対応画像とそれ以外に振り分ける
///
/// ファイルシステムへ問い合わせるため、UI スレッドから呼ばない
///
/// # Arguments
/// * `paths` - ドロップされたパス
///
/// # Returns
/// * `ScannedDrop` - 対応画像、除外件数 (読み取りに失敗したフォルダを含む)、フォルダの有無
fn scan_dropped_paths(paths: Vec<PathBuf>) -> ScannedDrop {
    let expanded = expand_dropped_dirs(paths);
    let (images, skipped) = partition_supported_images(expanded.paths);
    ScannedDrop {
        images,
        skipped: skipped + expanded.unreadable_dirs,
        has_dir: expanded.has_dir,
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

    use eframe::egui;

    use super::{expand_dropped_dirs, partition_supported_images};
    use crate::app::actions::test_helpers::{make_test_image, poll_io_until};
    use crate::app::state::{App, IoMsg, ScannedDrop};

    // ======================================================================
    // handle_dropped_paths() / partition_supported_images()
    // ======================================================================
    /// 対応拡張子のファイルのみを入力順のまま残し、除外件数を返すこと
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

    /// 画像の拡張子を持つフォルダを除外すること
    #[test]
    fn partition_supported_images_skips_directories() {
        let tmp = tempfile::Builder::new().suffix(".png").tempdir().unwrap();
        let (images, skipped) = partition_supported_images(vec![tmp.path().to_path_buf()]);

        assert_eq!(images, Vec::<PathBuf>::new());
        assert_eq!(skipped, 1);
    }

    /// 対応ファイルがない場合に件数をステータスへ表示し、読み込みを行わないこと
    #[test]
    fn handle_dropped_paths_reports_unsupported_only() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.handle_dropped_paths(&ctx, vec![PathBuf::from("a.txt"), PathBuf::from("b.pdf")]);
        // 展開はワーカースレッドで行うため、結果が届くまで待つ
        let done = poll_io_until(&mut app, &ctx, |a| a.status.contains("対応していない"));

        assert!(done, "status = {}", app.status);
        assert!(app.status.contains("2 件"), "status = {}", app.status);
        assert!(app.image.original.is_none());
    }

    /// 空のドロップではステータスを変更しないこと
    #[test]
    fn handle_dropped_paths_ignores_empty() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let status_before = app.status.clone();
        app.handle_dropped_paths(&ctx, Vec::new());
        assert_eq!(app.status, status_before);
        // ワーカーを起動しないこと
        assert!(app.io.rx.try_recv().is_err());
    }

    /// 対応画像が 1 件ならプレビューで開き、完了後のステータスにも除外件数を残すこと
    #[test]
    fn handle_dropped_paths_loads_single_image_with_note() {
        let ctx = egui::Context::default();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("input.png");
        make_test_image().save(&path).unwrap();

        let mut app = App::default();
        app.handle_dropped_paths(&ctx, vec![tmp.path().join("readme.txt"), path]);
        // 展開が終わり、読み込みを始めた時点で除外件数を表示する
        let loading = poll_io_until(&mut app, &ctx, |a| a.status.contains("読み込み中"));
        assert!(loading, "status = {}", app.status);
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

    /// 一括トリミングの実行中は複数ファイルのドロップを受け付けないこと
    #[test]
    fn handle_dropped_paths_rejects_batch_while_running() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.batch.running = true;
        app.handle_dropped_paths(&ctx, vec![PathBuf::from("a.png"), PathBuf::from("b.png")]);
        let done = poll_io_until(&mut app, &ctx, |a| a.status.contains("受け付けません"));
        assert!(done, "status = {}", app.status);
    }

    /// ダイアログ表示中の複数ファイルのドロップは、処理されなかったことをステータスに示すこと
    #[test]
    fn handle_dropped_paths_reports_discarded_while_dialog_open() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.io.dialog_open = true;
        app.handle_dropped_paths(&ctx, vec![PathBuf::from("a.png"), PathBuf::from("b.png")]);
        let done = poll_io_until(&mut app, &ctx, |a| {
            a.status.contains("処理できませんでした")
        });

        assert!(done, "status = {}", app.status);
        assert!(app.status.contains("2 件"), "status = {}", app.status);
        // 表示中のままであること
        assert!(app.io.dialog_open);
    }

    // ======================================================================
    // expand_dropped_dirs() / フォルダのドロップ
    // ======================================================================
    /// フォルダを直下のエントリへ名前順に展開し、ファイルは入力順のまま残すこと
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

    /// フォルダを含まない場合は入力をそのまま返すこと
    #[test]
    fn expand_dropped_dirs_keeps_files_as_is() {
        let paths = vec![PathBuf::from("b.png"), PathBuf::from("a.txt")];
        let expanded = expand_dropped_dirs(paths.clone());
        assert_eq!(expanded.paths, paths);
        assert!(!expanded.has_dir);
    }

    /// 対応画像のないフォルダでは専用のメッセージと除外件数を表示すること
    #[test]
    fn handle_dropped_paths_reports_folder_without_images() {
        let ctx = egui::Context::default();
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("sub")).unwrap();
        fs::write(tmp.path().join("readme.txt"), b"").unwrap();

        let mut app = App::default();
        app.handle_dropped_paths(&ctx, vec![tmp.path().to_path_buf()]);
        let done = poll_io_until(&mut app, &ctx, |a| {
            a.status.contains("フォルダに対応画像がありません")
        });

        // サブフォルダとテキストファイルの 2 件を除外として数える
        assert!(done, "status = {}", app.status);
        assert!(app.status.contains("2 件"), "status = {}", app.status);
        assert!(app.image.original.is_none());
    }

    /// 対応画像が 1 件だけのフォルダでもプレビューではなく一括トリミングとして扱うこと
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
        let done = poll_io_until(&mut app, &ctx, |a| a.status.contains("受け付けません"));

        assert!(done, "status = {}", app.status);
        assert!(app.io.load_note.is_none());
    }

    /// 展開中は UI スレッドを待たせず、確認中であることをステータスに示すこと
    #[test]
    fn handle_dropped_paths_shows_scanning_status_immediately() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.handle_dropped_paths(&ctx, vec![PathBuf::from("a.txt")]);

        assert!(app.status.contains("確認中"), "status = {}", app.status);
    }

    /// 後続のドロップで無効化された展開結果を無視すること
    #[test]
    fn poll_io_discards_stale_drop_scan() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let stale_id = app.io.drop_request_id;
        app.io.drop_request_id += 1;
        app.status = "確認中".into();
        app.io
            .tx
            .send(IoMsg::DropScanned {
                request_id: stale_id,
                result: Ok(ScannedDrop {
                    images: Vec::new(),
                    skipped: 3,
                    has_dir: false,
                }),
            })
            .unwrap();
        app.poll_io(&ctx);

        assert_eq!(app.status, "確認中");
    }

    /// 展開の失敗をステータスに表示すること
    #[test]
    fn poll_io_reports_drop_scan_failure() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.io
            .tx
            .send(IoMsg::DropScanned {
                request_id: app.io.drop_request_id,
                result: Err("失敗".into()),
            })
            .unwrap();
        app.poll_io(&ctx);

        assert_eq!(app.status, "失敗");
    }
}
