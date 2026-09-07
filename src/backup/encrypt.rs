//! Passphrase (scrypt) encryption via age, mirroring sqlite-vault. Files are
//! interchangeable with the `rage` CLI and sqlite-vault Go tooling.

use std::io::{Read, Write};

use age::secrecy::SecretString;

use crate::backup::BackupError;

/// Encrypt `plaintext` with an age scrypt recipient derived from `passphrase`.
pub fn encrypt(plaintext: &[u8], passphrase: &str) -> Result<Vec<u8>, BackupError> {
    let encryptor = age::Encryptor::with_user_passphrase(SecretString::from(passphrase.to_owned()));
    let mut out = Vec::new();
    let mut writer = encryptor.wrap_output(&mut out)?;
    writer.write_all(plaintext)?;
    let _ = writer.finish()?;
    Ok(out)
}

/// Decrypt an age file encrypted with `passphrase`.
pub fn decrypt(ciphertext: &[u8], passphrase: &str) -> Result<Vec<u8>, BackupError> {
    let decryptor = age::Decryptor::new_buffered(ciphertext)?;
    let identity = age::scrypt::Identity::new(SecretString::from(passphrase.to_owned()));
    let mut reader = decryptor.decrypt(std::iter::once(&identity as &dyn age::Identity))?;
    let mut out = Vec::new();
    reader.read_to_end(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let plaintext = b"hello kehrkraft \x00\xff\x01";
        let ciphertext = encrypt(plaintext, "correct horse battery staple").unwrap();
        assert_ne!(ciphertext, plaintext);
        assert_eq!(
            decrypt(&ciphertext, "correct horse battery staple").unwrap(),
            plaintext
        );
    }

    #[test]
    fn empty_plaintext_roundtrip() {
        let ciphertext = encrypt(b"", "pw").unwrap();
        assert_eq!(decrypt(&ciphertext, "pw").unwrap(), b"");
    }

    #[test]
    fn wrong_passphrase_is_rejected() {
        let ciphertext = encrypt(b"secret", "right").unwrap();
        assert!(decrypt(&ciphertext, "wrong").is_err());
    }

    #[test]
    fn not_an_age_file_is_rejected() {
        assert!(decrypt(b"not an age file", "pw").is_err());
    }
}
