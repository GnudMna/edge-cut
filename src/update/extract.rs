//! ポータブル ZIP の展開
//!
//! 展開先の外へ書き出すエントリ (Zip Slip) は飛ばし、展開直下が単一フォルダならそれを配布物のルートとする

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use zip::ZipArchive;

use super::error::UpdateError;

// ======================================================================
// 展開
// ======================================================================
/// ポータブル ZIP を展開し、配布物のルートを返す
///
/// 展開先が既にあれば作り直す
///
/// # Arguments
/// * `zip_path` - 検証済みの ZIP
/// * `extract_dir` - 展開先
///
/// # Returns
/// * `Result<PathBuf, UpdateError>` - 配布物のルート (展開直下の単一フォルダ、または展開先)
pub(crate) fn extract_portable_zip(
    zip_path: &Path,
    extract_dir: &Path,
) -> Result<PathBuf, UpdateError> {
    if let Err(err) = fs::remove_dir_all(extract_dir)
        && err.kind() != io::ErrorKind::NotFound
    {
        return Err(UpdateError::WorkDirCreate);
    }
    fs::create_dir_all(extract_dir).map_err(|_| UpdateError::WorkDirCreate)?;

    extract_zip_archive(zip_path, extract_dir)?;

    Ok(find_single_root(extract_dir).unwrap_or_else(|| extract_dir.to_path_buf()))
}

/// ZIP を展開する (展開先の外を指すエントリは飛ばす)
fn extract_zip_archive(zip_path: &Path, dest_dir: &Path) -> Result<(), UpdateError> {
    let file = File::open(zip_path).map_err(|_| UpdateError::ZipInvalid)?;
    let mut archive = ZipArchive::new(file).map_err(|_| UpdateError::ZipInvalid)?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| UpdateError::ZipExtract)?;
        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest_dir.join(enclosed);

        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|_| UpdateError::ZipExtract)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|_| UpdateError::ZipExtract)?;
        }
        let mut out_file = File::create(&out_path).map_err(|_| UpdateError::ZipExtract)?;
        io::copy(&mut entry, &mut out_file).map_err(|_| UpdateError::ZipExtract)?;
    }
    Ok(())
}

/// 展開先直下がフォルダ 1 つだけなら、そのフォルダを返す
fn find_single_root(extract_dir: &Path) -> Option<PathBuf> {
    let mut entries = fs::read_dir(extract_dir).ok()?.flatten();
    let first = entries.next()?;
    if entries.next().is_some() || !first.file_type().ok()?.is_dir() {
        return None;
    }
    Some(first.path())
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use super::*;

    /// 名前と内容の組から ZIP を作る
    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).expect("zip create");
        let mut writer = ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in entries {
            writer.start_file(*name, options).expect("start file");
            writer.write_all(body).expect("write");
        }
        writer.finish().expect("finish zip");
    }

    /// 入れ子のファイルを展開し、単一のルートフォルダを返すこと
    #[test]
    fn extract_returns_single_root_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let zip_path = tmp.path().join("pkg.zip");
        write_zip(
            &zip_path,
            &[("pkg/EdgeCut.exe", b"exe"), ("pkg/docs/index.html", b"doc")],
        );

        let root = extract_portable_zip(&zip_path, &tmp.path().join("out")).expect("extract");

        assert!(root.ends_with("pkg"));
        assert_eq!(fs::read(root.join("EdgeCut.exe")).expect("read"), b"exe");
        assert_eq!(
            fs::read(root.join("docs").join("index.html")).expect("read"),
            b"doc"
        );
    }

    /// 直下にファイルがある場合は展開先をそのままルートにすること
    #[test]
    fn extract_returns_extract_dir_when_flat() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let zip_path = tmp.path().join("flat.zip");
        write_zip(&zip_path, &[("EdgeCut.exe", b"exe")]);
        let out = tmp.path().join("out");

        let root = extract_portable_zip(&zip_path, &out).expect("extract");
        assert_eq!(root, out);
    }

    /// 展開先の外を指すエントリは書き出さないこと
    #[test]
    fn extract_skips_zip_slip_entries() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let zip_path = tmp.path().join("slip.zip");
        write_zip(
            &zip_path,
            &[("../evil.txt", b"evil"), ("safe.txt", b"safe")],
        );
        let out = tmp.path().join("out");

        extract_portable_zip(&zip_path, &out).expect("extract");
        assert!(out.join("safe.txt").is_file());
        assert!(!tmp.path().join("evil.txt").exists());
    }

    /// 前回の展開結果を消してから展開すること
    #[test]
    fn extract_replaces_previous_output() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let out = tmp.path().join("out");
        fs::create_dir_all(&out).expect("out");
        fs::write(out.join("stale.txt"), b"old").expect("stale");
        let zip_path = tmp.path().join("pkg.zip");
        write_zip(&zip_path, &[("EdgeCut.exe", b"exe")]);

        extract_portable_zip(&zip_path, &out).expect("extract");
        assert!(!out.join("stale.txt").exists());
    }

    /// ZIP でないファイルは形式不正として失敗すること
    #[test]
    fn extract_rejects_non_zip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("broken.zip");
        fs::write(&path, b"not a zip").expect("write");

        let result = extract_portable_zip(&path, &tmp.path().join("out"));
        assert_eq!(result, Err(UpdateError::ZipInvalid));
    }
}
