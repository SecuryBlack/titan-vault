#[test]
fn test_full_pipeline_roundtrip_integrity() {
    let original_content = b"SecuryBlack TitanVault Integrity Verification: 2026-09-18";

    // 1. Compress with zstd
    let compressed = zstd::encode_all(&original_content[..], 3).expect("zstd compression failed");
    assert!(compressed.len() > 0);

    // 2. Encrypt with ChaCha20-Poly1305
    let passphrase = "my-ultra-secure-passphrase";
    let mut hasher = sha2::Sha256::default();
    use sha2::Digest;
    hasher.update(passphrase.as_bytes());
    let key = hasher.finalize();

    use chacha20poly1305::aead::{Aead, KeyInit};
    let cipher = chacha20poly1305::ChaCha20Poly1305::new(&key);
    let nonce = chacha20poly1305::Nonce::from_slice(b"123456789012"); // 12 bytes
    let ciphertext = cipher.encrypt(nonce, compressed.as_slice()).expect("encryption failed");

    // 3. Decrypt
    let decrypted_compressed = cipher.decrypt(nonce, ciphertext.as_slice()).expect("decryption failed");
    assert_eq!(decrypted_compressed, compressed);

    // 4. Decompress with zstd
    let decompressed = zstd::decode_all(decrypted_compressed.as_slice()).expect("zstd decompression failed");
    assert_eq!(decompressed, original_content);
}
