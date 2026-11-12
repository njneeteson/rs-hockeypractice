//! Password hashing (Argon2id) and password rules.

use argon2::Argon2;
use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use std::sync::OnceLock;

pub const MIN_LENGTH: usize = 8;
const MAX_LENGTH: usize = 256;

pub fn hash(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("hashing password: {e}"))
}

pub fn verify(password: &str, stored_hash: &str) -> bool {
    PasswordHash::new(stored_hash).is_ok_and(|h| {
        Argon2::default()
            .verify_password(password.as_bytes(), &h)
            .is_ok()
    })
}

/// Spend the same time as a real check, so response timing doesn't reveal
/// whether a username exists.
pub fn verify_dummy(password: &str) {
    static DUMMY: OnceLock<String> = OnceLock::new();
    let h = DUMMY.get_or_init(|| hash("not-a-real-password").unwrap_or_default());
    let _ = verify(password, h);
}

/// Check a new password and its confirmation. Returns a message for the user on failure.
pub fn validate_new(password: &str, confirm: &str) -> Result<(), String> {
    if password.chars().count() < MIN_LENGTH {
        return Err(format!("Passwords need at least {MIN_LENGTH} characters."));
    }
    if password.len() > MAX_LENGTH {
        return Err("That password is too long.".into());
    }
    if password != confirm {
        return Err("The two passwords don't match.".into());
    }
    Ok(())
}

/// A random temporary password such as `kfmr-7wq2-pxne-4c9h`, for new accounts and resets.
pub fn generate_temporary() -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    let chars: Vec<char> = bytes
        .iter()
        .map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()] as char)
        .collect();
    chars
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let h = hash("hunter22!").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify("hunter22!", &h));
        assert!(!verify("hunter22?", &h));
        assert!(!verify("hunter22!", "garbage"));
    }

    #[test]
    fn rules() {
        assert!(validate_new("short", "short").is_err());
        assert!(validate_new("long enough", "long enougH").is_err());
        assert!(validate_new("long enough", "long enough").is_ok());
        let t = generate_temporary();
        assert_eq!(t.len(), 19);
        assert!(validate_new(&t, &t).is_ok());
    }
}
