//! ChaCha20-Poly1305 による埋め込みキーの暗号化・復号

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

pub(crate) const CIPHER_KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// `nonce || ciphertext` を復号する
///
/// # Arguments
/// * `key` - ChaCha20-Poly1305 鍵
/// * `blob` - `nonce || ciphertext`
///
/// # Returns
/// * `Option<Vec<u8>>` - 平文。形式不正または認証失敗なら `None`
pub(crate) fn decrypt_bytes(key: &[u8; CIPHER_KEY_LEN], blob: &[u8]) -> Option<Vec<u8>> {
    if blob.len() <= NONCE_LEN {
        return None;
    }
    let (nonce, ciphertext) = blob.split_at(NONCE_LEN);
    let key = Key::try_from(key.as_slice()).ok()?;
    let cipher = ChaCha20Poly1305::new(&key);
    let nonce = Nonce::try_from(nonce).ok()?;
    cipher.decrypt(&nonce, ciphertext).ok()
}

/// テスト用に固定 nonce で暗号化し、`nonce || ciphertext` を返す
///
/// # Arguments
/// * `key` - ChaCha20-Poly1305 鍵
/// * `plaintext` - 平文
///
/// # Returns
/// * `Option<Vec<u8>>` - `nonce || ciphertext`。鍵長不正または暗号化失敗なら `None`
#[cfg(test)]
pub(crate) fn encrypt_bytes(key: &[u8; CIPHER_KEY_LEN], plaintext: &[u8]) -> Option<Vec<u8>> {
    let key = Key::try_from(key.as_slice()).ok()?;
    let cipher = ChaCha20Poly1305::new(&key);
    let nonce = [7u8; NONCE_LEN];
    let nonce_arr = Nonce::try_from(nonce.as_slice()).ok()?;
    let ciphertext = cipher.encrypt(&nonce_arr, plaintext).ok()?;
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Some(out)
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: [u8; CIPHER_KEY_LEN] = [7; CIPHER_KEY_LEN];

    /// バイト列の暗号化往復が成功すること
    #[test]
    fn encrypt_bytes_roundtrip() {
        let plaintext = b"secret update key";
        let encrypted = encrypt_bytes(&TEST_KEY, plaintext).expect("encrypt");
        assert_ne!(encrypted.as_slice(), plaintext);
        let decrypted = decrypt_bytes(&TEST_KEY, &encrypted).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    /// 短すぎる暗号文は復号を拒否すること
    #[test]
    fn decrypt_bytes_rejects_too_short_blob() {
        assert!(decrypt_bytes(&TEST_KEY, &[0u8; NONCE_LEN]).is_none());
    }
}
