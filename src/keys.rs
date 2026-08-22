//! Session-only ephemeral key generation.

use ed25519_dalek::SigningKey as Ed25519SigningKey;
use p256::ecdsa::SigningKey as P256SigningKey;
use rand::rngs::OsRng;
use zeroize::Zeroize;

use crate::codec::to_hex;
use crate::ops::{ED25519_NAME, P256_NAME};

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
}
