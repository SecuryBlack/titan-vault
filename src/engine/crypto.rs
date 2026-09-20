use anyhow::{anyhow, Result};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::RngCore;
use sha2::{Digest, Sha256};

const MAGIC_HEADER: &[u8; 4] = b"TVLT"; // TitanVault Encrypted Archive Magic
const NONCE_SIZE: usize = 12;

pub struct CryptoEngine {
    cipher: ChaCha20Poly1305,
}

impl CryptoEngine {
    pub fn from_passphrase(passphrase: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(passphrase.as_bytes());
        let hash = hasher.finalize();
        let key = Key::from_slice(&hash);
        let cipher = ChaCha20Poly1305::new(key);
        Self { cipher }
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow!("encryption failed: {e}"))?;

        let mut output = Vec::with_capacity(MAGIC_HEADER.len() + NONCE_SIZE + ciphertext.len());
        output.extend_from_slice(MAGIC_HEADER);
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        Ok(output)
    }

    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < MAGIC_HEADER.len() + NONCE_SIZE {
            return Err(anyhow!("data too short to be a valid TitanVault encrypted payload"));
        }

        if &data[0..4] != MAGIC_HEADER {
            return Err(anyhow!("invalid magic header: not a TitanVault encrypted archive"));
        }

        let nonce_bytes = &data[4..4 + NONCE_SIZE];
        let ciphertext = &data[4 + NONCE_SIZE..];
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow!("decryption failed (wrong passphrase or corrupted payload): {e}"))?;

        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_roundtrip() {
        let engine = CryptoEngine::from_passphrase("super-secret-key-1234");
        let payload = b"Hello, SecuryBlack TitanVault Backup!";

        let encrypted = engine.encrypt(payload).expect("encryption failed");
        assert_ne!(&encrypted[..], payload);
        assert_eq!(&encrypted[0..4], b"TVLT");

        let decrypted = engine.decrypt(&encrypted).expect("decryption failed");
        assert_eq!(&decrypted[..], payload);
    }
}
