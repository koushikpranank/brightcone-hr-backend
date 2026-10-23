use ring::rand::{self, SecureRandom};
use ring::{
    aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM},
    hmac,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct KeyConfig {
    active: String,
    keys: std::collections::HashMap<String, String>,
    lookup_key: String,
}

fn config() -> Result<KeyConfig, String> {
    let keys: std::collections::HashMap<String, String> = serde_json::from_str(
        &std::env::var("PEOPLE_DATA_KEYS_JSON")
            .map_err(|_| "People data encryption is not configured".to_owned())?,
    )
    .map_err(|_| "People data encryption is not configured".to_owned())?;
    let active = std::env::var("PEOPLE_DATA_ACTIVE_KEY_ID")
        .map_err(|_| "People data encryption is not configured".to_owned())?;
    let lookup_key = std::env::var("PEOPLE_EMAIL_LOOKUP_KEY_HEX")
        .map_err(|_| "People data encryption is not configured".to_owned())?;
    let cfg = KeyConfig {
        active,
        keys,
        lookup_key,
    };
    if cfg.active.is_empty()
        || cfg.active.contains(':')
        || cfg.active.len() > 64
        || cfg
            .keys
            .values()
            .any(|v| decode_hex(v).map_or(true, |key| key.len() != 32))
        || cfg
            .keys
            .get(&cfg.active)
            .and_then(|v| decode_hex(v).ok())
            .is_none_or(|v| v.len() != 32)
        || decode_hex(&cfg.lookup_key).map_or(true, |v| v.len() < 32)
    {
        return Err("People data encryption is not configured".to_owned());
    }
    Ok(cfg)
}

pub fn encrypt(field: &str, value: &str) -> Result<String, String> {
    let cfg = config()?;
    let key_bytes = decode_hex(&cfg.keys[&cfg.active])?;
    let key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, &key_bytes).map_err(|_| "Encryption failed")?,
    );
    let mut nonce_bytes = [0_u8; 12];
    rand::SystemRandom::new()
        .fill(&mut nonce_bytes)
        .map_err(|_| "Encryption failed")?;
    let mut ciphertext = value.as_bytes().to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce_bytes),
        Aad::from(field.as_bytes()),
        &mut ciphertext,
    )
    .map_err(|_| "Encryption failed")?;
    Ok(format!(
        "v1:{}:{}:{}",
        cfg.active,
        encode_hex(&nonce_bytes),
        encode_hex(&ciphertext)
    ))
}

pub fn decrypt(field: &str, value: &str) -> Result<String, String> {
    let cfg = config()?;
    let mut parts = value.split(':');
    if parts.next() != Some("v1") {
        return Err("Encrypted People data is invalid".to_owned());
    }
    let key_id = parts.next().ok_or("Encrypted People data is invalid")?;
    let nonce = decode_hex(parts.next().ok_or("Encrypted People data is invalid")?)?;
    let mut ciphertext = decode_hex(parts.next().ok_or("Encrypted People data is invalid")?)?;
    if parts.next().is_some() || nonce.len() != 12 {
        return Err("Encrypted People data is invalid".to_owned());
    }
    let key_bytes = cfg
        .keys
        .get(key_id)
        .ok_or("Encrypted People key is unavailable")?;
    let key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, &decode_hex(key_bytes)?)
            .map_err(|_| "Encrypted People key is invalid")?,
    );
    let plain = key
        .open_in_place(
            Nonce::assume_unique_for_key(
                nonce
                    .try_into()
                    .map_err(|_| "Encrypted People data is invalid")?,
            ),
            Aad::from(field.as_bytes()),
            &mut ciphertext,
        )
        .map_err(|_| "Encrypted People data failed authentication")?;
    String::from_utf8(plain.to_vec()).map_err(|_| "Encrypted People data is invalid".to_owned())
}

/// Re-encrypts one field with the configured active key. Reads may use a retained key.
pub fn rotate(field: &str, value: &str) -> Result<String, String> {
    let plaintext = decrypt(field, value)?;
    encrypt(field, &plaintext)
}

pub fn email_lookup(value: &str) -> Result<String, String> {
    let cfg = config()?;
    let key = hmac::Key::new(hmac::HMAC_SHA256, &decode_hex(&cfg.lookup_key)?);
    Ok(encode_hex(
        hmac::sign(&key, value.trim().to_lowercase().as_bytes()).as_ref(),
    ))
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err("Encryption configuration is invalid".to_owned());
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Ok(byte - b'0'),
                b'a'..=b'f' => Ok(byte - b'a' + 10),
                b'A'..=b'F' => Ok(byte - b'A' + 10),
                _ => Err("Encryption configuration is invalid".to_owned()),
            };
            Ok(digit(pair[0])? * 16 + digit(pair[1])?)
        })
        .collect()
}

fn encode_hex(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 15) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn field_binding_and_key_rotation_are_supported() {
        let previous = [
            (
                "PEOPLE_DATA_ACTIVE_KEY_ID",
                std::env::var_os("PEOPLE_DATA_ACTIVE_KEY_ID"),
            ),
            (
                "PEOPLE_DATA_KEYS_JSON",
                std::env::var_os("PEOPLE_DATA_KEYS_JSON"),
            ),
            (
                "PEOPLE_EMAIL_LOOKUP_KEY_HEX",
                std::env::var_os("PEOPLE_EMAIL_LOOKUP_KEY_HEX"),
            ),
        ];
        std::env::set_var("PEOPLE_DATA_ACTIVE_KEY_ID", "old-key");
        std::env::set_var(
            "PEOPLE_DATA_KEYS_JSON",
            format!(
                r#"{{"old-key":"{}","new-key":"{}"}}"#,
                "11".repeat(32),
                "22".repeat(32)
            ),
        );
        std::env::set_var("PEOPLE_EMAIL_LOOKUP_KEY_HEX", "33".repeat(32));

        let ciphertext = encrypt("employee.first_name", "Sample").expect("encrypt");
        assert_eq!(
            decrypt("employee.first_name", &ciphertext).expect("decrypt"),
            "Sample"
        );
        assert!(decrypt("employee.last_name", &ciphertext).is_err());

        std::env::set_var("PEOPLE_DATA_ACTIVE_KEY_ID", "new-key");
        assert_eq!(
            decrypt("employee.first_name", &ciphertext).expect("old key read"),
            "Sample"
        );
        let rotated = rotate("employee.first_name", &ciphertext).expect("rotate");
        assert!(rotated.starts_with("v1:new-key:"));
        std::env::set_var(
            "PEOPLE_DATA_KEYS_JSON",
            r#"{"new-key":"2222222222222222222222222222222222222222222222222222222222222222"}"#,
        );
        assert!(decrypt("employee.first_name", &ciphertext).is_err());

        for (name, value) in previous {
            if let Some(value) = value {
                std::env::set_var(name, value);
            } else {
                std::env::remove_var(name);
            }
        }
    }
}
