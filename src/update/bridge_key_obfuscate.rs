//! `build.rs` と実行時で共有する XOR 難読化

/// XOR マスク長 (ChaCha20-Poly1305 鍵長と同じ)
pub(crate) const OBFUSCATE_MASK_LEN: usize = 32;

/// `data` をマスクで可逆 XOR する
///
/// # Arguments
/// * `data` - 難読化 / 復元するバッファ
/// * `mask` - XOR マスク
pub(crate) fn xor_obfuscate(data: &mut [u8], mask: &[u8; OBFUSCATE_MASK_LEN]) {
    for (i, byte) in data.iter_mut().enumerate() {
        let m1 = mask[i % OBFUSCATE_MASK_LEN];
        let m2 = mask[i.wrapping_mul(7).wrapping_add(13) % OBFUSCATE_MASK_LEN];
        let mix = u8::try_from(i & 0xff).unwrap_or(0);
        *byte ^= m1 ^ m2 ^ mix.wrapping_mul(0x5d);
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// XOR 難読化が対合であり平文をそのまま残さないこと
    #[test]
    fn xor_obfuscate_is_involution_and_hides_plaintext() {
        let mask = [3u8; OBFUSCATE_MASK_LEN];
        let original = b"hello-bridge-key".to_vec();
        let mut data = original.clone();
        xor_obfuscate(&mut data, &mask);
        assert_ne!(data, original);
        xor_obfuscate(&mut data, &mask);
        assert_eq!(data, original);
    }
}
