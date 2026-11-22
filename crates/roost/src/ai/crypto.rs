use std::fs;
use std::path::Path;

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};

/// Cifrador de secretos usando `chacha20poly1305` y `<data_dir>/secret.key` (permisos `0600`).
#[derive(Clone)]
pub struct SecretCipher {
    key: [u8; 32],
}

impl SecretCipher {
    /// Carga `secret.key` o lo crea con 32 bytes aleatorios y permisos `0600`.
    pub fn load_or_create(path: &Path) -> Result<Self, String> {
        if path.is_file() {
            let bytes = fs::read(path).map_err(|e| format!("no se pudo leer secret.key: {e}"))?;
            if bytes.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                return Ok(Self { key });
            }
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("no se pudo crear carpeta para secret.key: {e}"))?;
        }

        let mut key = [0u8; 32];
        getrandom::getrandom(&mut key)
            .map_err(|e| format!("no se pudo generar secret.key: {e}"))?;
        fs::write(path, key).map_err(|e| format!("no se pudo escribir secret.key: {e}"))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }

        Ok(Self { key })
    }

    /// Crea una clave temporal en memoria (para modo demo y tests).
    pub fn ephemeral() -> Self {
        let mut key = [0u8; 32];
        let _ = getrandom::getrandom(&mut key);
        Self { key }
    }

    /// Cifra un texto y devuelve `hex(nonce || ciphertext)`.
    pub fn encrypt(&self, plaintext: &str) -> Result<String, String> {
        let cipher = ChaCha20Poly1305::new((&self.key).into());
        let mut nonce_bytes = [0u8; 12];
        getrandom::getrandom(&mut nonce_bytes).map_err(|e| e.to_string())?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|_| "error al cifrar secreto".to_string())?;

        let mut combined = Vec::with_capacity(12 + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);
        Ok(hex::encode(combined))
    }

    /// Descifra un secreto previamente cifrado con [`Self::encrypt`].
    pub fn decrypt(&self, encoded: &str) -> Result<String, String> {
        let raw = hex::decode(encoded).map_err(|_| "secreto corrupto".to_string())?;
        if raw.len() < 13 {
            return Err("secreto incompleto".to_string());
        }
        let cipher = ChaCha20Poly1305::new((&self.key).into());
        let nonce = Nonce::from_slice(&raw[..12]);
        let plain = cipher
            .decrypt(nonce, &raw[12..])
            .map_err(|_| "no se pudo descifrar el secreto".to_string())?;
        String::from_utf8(plain).map_err(|_| "secreto inválido".to_string())
    }
}

/// Devuelve solo los últimos 4 caracteres visibles de una clave (nunca la clave completa).
pub fn mask_last4(secret: &str) -> String {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let keep = chars.len().min(4);
    chars[chars.len() - keep..].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip_and_mask() {
        let cipher = SecretCipher::ephemeral();
        let enc = cipher.encrypt("sk-or-v1-supersecret9876").unwrap();
        assert!(!enc.contains("supersecret"));
        assert_eq!(cipher.decrypt(&enc).unwrap(), "sk-or-v1-supersecret9876");
        assert_eq!(mask_last4("sk-or-v1-supersecret9876"), "9876");
    }
}
