//! UI を配信するカスタムプロトコル (`edgecut://`)
//!
//! ビルド時に埋め込んだ UI のファイルと、プレビュー画像の RGBA を返す。
//! `WebView2` では `http://edgecut.localhost/` として見える

use std::borrow::Cow;
use std::sync::{Arc, Mutex, PoisonError};

use image::RgbaImage;
use wry::http::{Request, Response, StatusCode, header};

/// カスタムプロトコルの名前
pub(super) const SCHEME: &str = "edgecut";

/// UI のオリジン (`WebView2` はカスタムプロトコルを `http://{名前}.localhost` として扱う)
pub(super) const ORIGIN: &str = "http://edgecut.localhost";

/// 起動時に開くページ
pub(super) const START_URL: &str = "http://edgecut.localhost/index.html";

/// 更新プログラムが開くページ (UI はクエリで更新画面に切り替える)
pub(super) const UPDATER_URL: &str = "http://edgecut.localhost/index.html?view=updater";

/// プレビュー画像の取得パスの接頭辞 (`/preview/{id}`)
const PREVIEW_PREFIX: &str = "/preview/";

// build.rs が生成する。`UI_ASSETS: &[(&str, &str, &[u8])]` (パス, MIME タイプ, 内容)
include!(concat!(env!("OUT_DIR"), "/ui_assets.rs"));

// ======================================================================
// プレビュー画像の共有
// ======================================================================
/// UI へ渡すプレビュー画像 (識別子, 画像)
///
/// イベントループが差し替え、カスタムプロトコルの処理が読む
#[derive(Clone, Default)]
pub(super) struct PreviewSlot(Arc<Mutex<Option<SharedPreview>>>);

/// 共有するプレビュー画像 (識別子, 画像)
type SharedPreview = (u64, Arc<RgbaImage>);

impl PreviewSlot {
    /// プレビュー画像を差し替える
    ///
    /// # Arguments
    /// * `id` - 識別子
    /// * `image` - プレビュー画像 (`None` で破棄する)
    pub(super) fn set(&self, id: u64, image: Option<Arc<RgbaImage>>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = image.map(|img| (id, img));
    }

    /// 識別子が一致するプレビュー画像を返す
    fn get(&self, id: u64) -> Option<Arc<RgbaImage>> {
        let slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        slot.as_ref()
            .filter(|(current, _)| *current == id)
            .map(|(_, img)| Arc::clone(img))
    }

    /// 現在の識別子
    ///
    /// # Returns
    /// * `Option<u64>` - プレビュー画像が無ければ `None`
    pub(super) fn id(&self) -> Option<u64> {
        let slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        slot.as_ref().map(|(id, _)| *id)
    }
}

// ======================================================================
// 応答
// ======================================================================
/// カスタムプロトコルの要求に応答する
///
/// # Arguments
/// * `preview` - プレビュー画像
/// * `request` - `WebView` からの要求
///
/// # Returns
/// * `Response<Cow<'static, [u8]>>` - 応答。該当が無ければ 404
pub(super) fn respond(
    preview: &PreviewSlot,
    request: &Request<Vec<u8>>,
) -> Response<Cow<'static, [u8]>> {
    let path = request.uri().path();
    if let Some(id) = path.strip_prefix(PREVIEW_PREFIX) {
        return preview_response(preview, id);
    }
    let path = if path == "/" { "/index.html" } else { path };
    match find_asset(path) {
        Some((mime, body)) => Response::builder()
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Cow::Borrowed(body))
            .unwrap_or_else(|_| not_found()),
        None => not_found(),
    }
}

/// 埋め込みの UI ファイルを探す
///
/// # Arguments
/// * `path` - 要求パス (`/` で始まる)
///
/// # Returns
/// * `Option<(&str, &[u8])>` - MIME タイプと内容
fn find_asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    let path = path.strip_prefix('/')?;
    UI_ASSETS
        .iter()
        .find(|(p, _, _)| *p == path)
        .map(|(_, mime, body)| (*mime, *body))
}

/// プレビュー画像の RGBA を返す
///
/// 寸法は `X-Width` / `X-Height` ヘッダで渡す (アルファは乗算しない)
///
/// # Arguments
/// * `preview` - プレビュー画像
/// * `id` - 要求された識別子 (差し替え後の古い要求は 404 にする)
fn preview_response(preview: &PreviewSlot, id: &str) -> Response<Cow<'static, [u8]>> {
    let Some(image) = id.parse().ok().and_then(|id| preview.get(id)) else {
        return not_found();
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Width", image.width())
        .header("X-Height", image.height())
        .body(Cow::Owned(image.as_raw().clone()))
        .unwrap_or_else(|_| not_found())
}

/// 404 の応答
fn not_found() -> Response<Cow<'static, [u8]>> {
    let mut response = Response::new(Cow::Borrowed(&b""[..]));
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn get(preview: &PreviewSlot, path: &str) -> Response<Cow<'static, [u8]>> {
        let request = Request::builder()
            .uri(format!("{ORIGIN}{path}"))
            .body(Vec::new())
            .unwrap();
        respond(preview, &request)
    }

    /// 埋め込みの `index.html` を返すこと
    #[test]
    fn serves_index_html() {
        let slot = PreviewSlot::default();
        for path in ["/", "/index.html"] {
            let response = get(&slot, path);
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html");
            assert!(!response.body().is_empty());
        }
    }

    /// 存在しないパスは 404 を返すこと
    #[test]
    fn unknown_path_is_not_found() {
        let response = get(&PreviewSlot::default(), "/nope.js");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    /// 識別子が一致するときだけプレビュー画像を返すこと
    #[test]
    fn serves_preview_only_for_current_id() {
        let slot = PreviewSlot::default();
        slot.set(3, Some(Arc::new(RgbaImage::new(2, 1))));

        let response = get(&slot, "/preview/3");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["X-Width"], "2");
        assert_eq!(response.headers()["X-Height"], "1");
        assert_eq!(response.body().len(), 2 * 4);

        // 古い識別子・不正な識別子は 404
        assert_eq!(get(&slot, "/preview/2").status(), StatusCode::NOT_FOUND);
        assert_eq!(get(&slot, "/preview/x").status(), StatusCode::NOT_FOUND);
        slot.set(4, None);
        assert_eq!(slot.id(), None);
    }
}
