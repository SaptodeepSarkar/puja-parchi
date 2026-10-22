//! Photo / audio storage. Everything for one receipt book lives in
//! `data/photos/<book>/` and `data/audio/<book>/`, which is the folder layout the
//! user asked for.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Stored {
    pub path: PathBuf,
    pub url: String,
}

/// `data/photos/18/...` - the user asked for one folder per book number.
fn book_dir(book: i64) -> String {
    format!("{book}")
}

fn stamp() -> String {
    let now = chrono::Local::now();
    let rnd = uuid::Uuid::new_v4().simple().to_string();
    format!("{}-{}", now.format("%Y%m%d-%H%M%S"), &rnd[..6])
}

pub fn safe_ext(mime: &str, fallback: &str) -> String {
    let m = mime.to_ascii_lowercase();
    let ext = match m.as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/heic" => "heic",
        "image/heif" => "heif",
        "audio/webm" | "audio/webm;codecs=opus" => "webm",
        "audio/ogg" => "ogg",
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => "m4a",
        "audio/wav" | "audio/x-wav" => "wav",
        _ => fallback,
    };
    ext.to_string()
}

fn save(dir: &Path, kind: &str, book: i64, bytes: &[u8], ext: &str) -> std::io::Result<Stored> {
    let folder = dir.join(kind).join(book_dir(book));
    std::fs::create_dir_all(&folder)?;
    let name = format!("{}.{}", stamp(), ext);
    let path = folder.join(&name);
    std::fs::write(&path, bytes)?;
    let url = format!("/media/{kind}/{}/{name}", book_dir(book));
    Ok(Stored { path, url })
}

pub fn save_photo(data_dir: &Path, book: i64, bytes: &[u8], mime: &str) -> std::io::Result<Stored> {
    save(data_dir, "photos", book, bytes, &safe_ext(mime, "jpg"))
}

pub fn save_audio(data_dir: &Path, book: i64, bytes: &[u8], mime: &str) -> std::io::Result<Stored> {
    save(data_dir, "audio", book, bytes, &safe_ext(mime, "webm"))
}

/// Very small guard against a 0-byte or obviously-not-an-image upload.
pub fn looks_like_image(bytes: &[u8]) -> bool {
    if bytes.len() < 64 {
        return false;
    }
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])            // jpeg
        || bytes.starts_with(&[0x89, b'P', b'N', b'G']) // png
        || (bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP")
        || (bytes.len() > 12 && &bytes[4..8] == b"ftyp") // heic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jpeg_is_an_image() {
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0];
        v.extend(std::iter::repeat(0u8).take(100));
        assert!(looks_like_image(&v));
        assert!(!looks_like_image(b"hello"));
    }

    #[test]
    fn ext_mapping_is_whitelisted() {
        assert_eq!(safe_ext("image/jpeg", "jpg"), "jpg");
        assert_eq!(safe_ext("IMAGE/PNG", "jpg"), "png");
        assert_eq!(safe_ext("application/pdf", "jpg"), "jpg");
        assert_eq!(safe_ext("audio/webm;codecs=opus", "webm"), "webm");
    }

    #[test]
    fn book_folder_is_the_book_number() {
        assert_eq!(book_dir(18), "18");
    }
}
