//! Polkadot 19-circle identicons from `plot_icon` (SVG / WASM-safe).

use crate::codec::decode_key_bytes;

pub fn svg_for_key(algorithm: &str, public_key: &str) -> Option<String> {
    let bytes = decode_key_bytes(public_key).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let mut seed = Vec::with_capacity(algorithm.len() + 1 + bytes.len());
    seed.extend_from_slice(algorithm.as_bytes());
    seed.push(0);
    seed.extend_from_slice(&bytes);
    Some(plot_icon::generate_svg(&seed).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::generate_keypair;
    use crate::ops::ED25519_NAME;

    #[test]
    fn svg_for_ed25519_pubkey_is_nonempty() {
        let pair = generate_keypair(ED25519_NAME).expect("mint");
        let svg = svg_for_key(ED25519_NAME, &pair.public_hex).expect("svg");
        assert!(svg.contains("<svg"));
        assert!(svg.contains("<circle"));
        let other = generate_keypair(ED25519_NAME).expect("mint");
        let other_svg = svg_for_key(ED25519_NAME, &other.public_hex).expect("svg");
        assert_ne!(svg, other_svg);
    }

    #[test]
    fn empty_or_garbage_key_has_no_icon() {
        assert!(svg_for_key(ED25519_NAME, "").is_none());
        assert!(svg_for_key(ED25519_NAME, "not-a-key").is_none());
    }
}
