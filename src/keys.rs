//! Session-only ephemeral key generation.

use ed25519_dalek::SigningKey as Ed25519SigningKey;
use p256::ecdsa::SigningKey as P256SigningKey;
use rand::rngs::OsRng;
use zeroize::Zeroize;

use crate::codec::{decode_key_bytes, to_hex};
use crate::ops::{ED25519_NAME, P256_NAME};
use yaml_sigil_verification::{resolve_ed25519_verifying_key, resolve_p256_verifying_key};

pub struct KeyPairHex {
    pub private_hex: String,
    pub public_hex: String,
}

pub fn generate_keypair(algorithm: &str) -> Result<KeyPairHex, String> {
    match algorithm {
        ED25519_NAME => {
            let signing = Ed25519SigningKey::generate(&mut OsRng);
            let mut private = signing.to_bytes();
            let public = signing.verifying_key().to_bytes();
            let pair = KeyPairHex {
                private_hex: to_hex(&private),
                public_hex: to_hex(&public),
            };
            private.zeroize();
            Ok(pair)
        }
        P256_NAME => {
            let signing = P256SigningKey::random(&mut OsRng);
            let mut private = signing.to_bytes().to_vec();
            let public = signing.verifying_key().to_encoded_point(true);
            let pair = KeyPairHex {
                private_hex: to_hex(&private),
                public_hex: to_hex(public.as_bytes()),
            };
            private.zeroize();
            Ok(pair)
        }
        _ => Err("unsupported algorithm".into()),
    }
}

pub fn canonical_public_key(algorithm: &str, public_key: &str) -> Result<String, String> {
    let bytes = decode_key_bytes(public_key)?;
    match algorithm {
        ED25519_NAME => {
            let key = resolve_ed25519_verifying_key(&bytes).map_err(|_| {
                "public key is not a valid ED25519_PUREEDDSA_RAW_RS64_CANONICAL key".to_string()
            })?;
            Ok(to_hex(key.as_bytes()))
        }
        P256_NAME => {
            let key = resolve_p256_verifying_key(&bytes).map_err(|_| {
                "public key is not a valid ECDSA_SECP256R1_SHA256_RAW_RS64 key".to_string()
            })?;
            Ok(to_hex(key.to_encoded_point(true).as_bytes()))
        }
        _ => Err("unsupported algorithm".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_ephemeral_ed25519_pair() {
        let pair = generate_keypair(ED25519_NAME).expect("ed25519");
        assert_eq!(pair.private_hex.len(), 64);
        assert_eq!(pair.public_hex.len(), 64);
        let other = generate_keypair(ED25519_NAME).expect("ed25519");
        assert_ne!(pair.private_hex, other.private_hex);
    }

    #[test]
    fn mint_ephemeral_p256_pair() {
        let pair = generate_keypair(P256_NAME).expect("p256");
        assert_eq!(pair.private_hex.len(), 64);
        assert!(pair.public_hex.len() == 66 || pair.public_hex.len() == 130);
        assert!(generate_keypair("not-an-alg").is_err());
    }

    #[test]
    fn canonical_public_key_rejects_garbage() {
        let pair = generate_keypair(ED25519_NAME).expect("ed25519");
        let canonical = canonical_public_key(ED25519_NAME, &pair.public_hex).expect("canonical");
        assert_eq!(canonical, pair.public_hex);
        assert!(canonical_public_key(ED25519_NAME, "").is_err());
        assert!(canonical_public_key(ED25519_NAME, "not-a-key").is_err());
        assert!(canonical_public_key(ED25519_NAME, "00").is_err());
        assert!(canonical_public_key(P256_NAME, &pair.public_hex).is_err());
    }
}
