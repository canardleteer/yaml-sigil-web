//! Hex and base64 helpers for playground text fields.

use base64::Engine;

pub fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn decode_key_bytes(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty key material".into());
    }
    if let Ok(bytes) = decode_hex(trimmed) {
        return Ok(bytes);
    }
    decode_base64(trimmed).map_err(|_| "key must be hex (optional 0x) or base64".into())
}

/// Unsigned YAML payload: textarea UTF-8, independent of artifact form.
pub fn decode_yaml_text(input: &str) -> Vec<u8> {
    input.as_bytes().to_vec()
}

pub fn encode_yaml_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}

/// Artifact or signature-carrier wire: YAML is text, protobuf is base64.
pub fn decode_payload(input: &str, form: &str) -> Result<Vec<u8>, String> {
    match form {
        "yaml" => Ok(decode_yaml_text(input)),
        "protobuf" => decode_binary_field(input),
        _ => Err("form must be yaml or protobuf".into()),
    }
}

pub fn encode_payload(bytes: &[u8], form: &str) -> String {
    match form {
        "protobuf" => base64::engine::general_purpose::STANDARD.encode(bytes),
        _ => encode_yaml_text(bytes),
    }
}

pub fn decode_binary_field(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if let Ok(bytes) = decode_hex(trimmed) {
        return Ok(bytes);
    }
    decode_base64(trimmed)
}

fn decode_hex(input: &str) -> Result<Vec<u8>, ()> {
    let mut s = input.trim();
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        s = rest;
    }
    let compact: String = s
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':')
        .collect();
    if compact.is_empty() || !compact.len().is_multiple_of(2) {
        return Err(());
    }
    if !compact.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(());
    }
    (0..compact.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&compact[i..i + 2], 16).map_err(|_| ()))
        .collect()
}

fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let compact: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(&compact)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&compact))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&compact))
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(&compact))
        .map_err(|_| "invalid base64".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let bytes = [0x0a, 0xb1, 0xff];
        assert_eq!(to_hex(&bytes), "0ab1ff");
        assert_eq!(decode_hex("0x0a:b1 ff").unwrap(), bytes);
        assert_eq!(decode_hex("0AB1FF").unwrap(), bytes);
    }

    #[test]
    fn key_bytes_accept_hex_and_base64() {
        let bytes = vec![1, 2, 3, 4];
        let hex = to_hex(&bytes);
        assert_eq!(decode_key_bytes(&hex).unwrap(), bytes);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        assert_eq!(decode_key_bytes(&b64).unwrap(), bytes);
        assert!(decode_key_bytes("   ").is_err());
    }

    #[test]
    fn payload_yaml_is_utf8_protobuf_is_binary() {
        assert_eq!(decode_payload("a: 1\n", "yaml").unwrap(), b"a: 1\n");
        assert_eq!(decode_yaml_text("a: 1\n"), b"a: 1\n");
        let encoded = encode_payload(&[0xff, 0x00], "protobuf");
        assert_eq!(decode_payload(&encoded, "protobuf").unwrap(), [0xff, 0x00]);
        assert!(decode_payload("x", "cbor").is_err());
    }

    #[test]
    fn empty_protobuf_field_is_empty_bytes() {
        assert!(decode_binary_field("").unwrap().is_empty());
    }
}
