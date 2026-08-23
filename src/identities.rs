//! Session-only named identities for the playground.

use rand::Rng;
use rand::rngs::OsRng;
use rand::seq::SliceRandom;

use crate::keys::{canonical_public_key, generate_keypair};
use crate::ops::{ED25519_NAME, P256_NAME};

pub const PRESET_IDS: [&str; 3] = ["alice", "bob", "carol"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub id: String,
    pub label: String,
    pub algorithm: String,
    pub private_hex: String,
    pub public_hex: String,
    pub preset: bool,
    pub verify_only: bool,
}

impl Identity {
    pub fn display_name(&self) -> String {
        if self.verify_only {
            format!("{} (verify only)", self.label)
        } else {
            self.label.clone()
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Roster {
    identities: Vec<Identity>,
}

impl Roster {
    pub fn seed() -> Result<Self, String> {
        Self::seed_with_algs(assign_preset_algs(&mut OsRng))
    }

    fn seed_with_algs(algs: [String; 3]) -> Result<Self, String> {
        let mut identities = Vec::with_capacity(PRESET_IDS.len());
        for (id, algorithm) in PRESET_IDS.iter().zip(algs.iter()) {
            let pair = generate_keypair(algorithm)?;
            identities.push(Identity {
                id: (*id).to_string(),
                label: (*id).to_string(),
                algorithm: algorithm.clone(),
                private_hex: pair.private_hex,
                public_hex: pair.public_hex,
                preset: true,
                verify_only: false,
            });
        }
        Ok(Self { identities })
    }

    pub fn list(&self) -> &[Identity] {
        &self.identities
    }

    pub fn get(&self, id: &str) -> Option<&Identity> {
        self.identities.iter().find(|identity| identity.id == id)
    }

    pub fn add(&mut self, label: &str, algorithm: &str) -> Result<String, String> {
        let id = self.prepare_new_id(label)?;
        let pair = generate_keypair(algorithm)?;
        self.identities.push(Identity {
            id: id.clone(),
            label: label.trim().to_string(),
            algorithm: algorithm.to_string(),
            private_hex: pair.private_hex,
            public_hex: pair.public_hex,
            preset: false,
            verify_only: false,
        });
        Ok(id)
    }

    pub fn mint_verify_only(&mut self, label: &str, algorithm: &str) -> Result<String, String> {
        let id = self.prepare_new_id(label)?;
        let pair = generate_keypair(algorithm)?;
        self.identities.push(Identity {
            id: id.clone(),
            label: label.trim().to_string(),
            algorithm: algorithm.to_string(),
            private_hex: String::new(),
            public_hex: pair.public_hex,
            preset: false,
            verify_only: true,
        });
        Ok(id)
    }

    pub fn add_verify_only(
        &mut self,
        label: &str,
        algorithm: &str,
        public_hex: &str,
    ) -> Result<String, String> {
        let public_hex = canonical_public_key(algorithm, public_hex)?;
        let id = self.prepare_new_id(label)?;
        self.identities.push(Identity {
            id: id.clone(),
            label: label.trim().to_string(),
            algorithm: algorithm.to_string(),
            private_hex: String::new(),
            public_hex,
            preset: false,
            verify_only: true,
        });
        Ok(id)
    }

    fn prepare_new_id(&self, label: &str) -> Result<String, String> {
        let label = label.trim();
        if label.is_empty() {
            return Err("name is required".into());
        }
        let id = slugify(label);
        if id.is_empty() {
            return Err("name must include a letter or digit".into());
        }
        if let Some(existing) = self
            .identities
            .iter()
            .find(|identity| identity.label.eq_ignore_ascii_case(label) || identity.id == id)
        {
            return Err(format!(
                "identity '{}' already exists (names are unique ignoring case)",
                existing.label
            ));
        }
        Ok(id)
    }

    pub fn remint(&mut self, id: &str, algorithm: &str) -> Result<(), String> {
        let identity = self
            .identities
            .iter_mut()
            .find(|identity| identity.id == id)
            .ok_or_else(|| format!("unknown identity '{id}'"))?;
        if identity.preset {
            return Err("alice, bob, and carol keep the keys minted at load".into());
        }
        if identity.verify_only {
            return Err("verify-only identities do not mint private keys".into());
        }
        let pair = generate_keypair(algorithm)?;
        identity.algorithm = algorithm.to_string();
        identity.private_hex = pair.private_hex;
        identity.public_hex = pair.public_hex;
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let Some(identity) = self.get(id) else {
            return Err(format!("unknown identity '{id}'"));
        };
        if identity.preset {
            return Err("alice, bob, and carol cannot be removed".into());
        }
        self.identities.retain(|identity| identity.id != id);
        Ok(())
    }

    pub fn update_keys(
        &mut self,
        id: &str,
        private_hex: &str,
        public_hex: &str,
    ) -> Result<(), String> {
        let identity = self
            .identities
            .iter_mut()
            .find(|identity| identity.id == id)
            .ok_or_else(|| format!("unknown identity '{id}'"))?;
        if identity.preset {
            return Err("alice, bob, and carol keys are read-only".into());
        }
        identity.private_hex = private_hex.to_string();
        identity.public_hex = public_hex.to_string();
        Ok(())
    }
}

pub fn assign_preset_algs(rng: &mut impl Rng) -> [String; 3] {
    let extra = if rng.gen_bool(0.5) {
        ED25519_NAME
    } else {
        P256_NAME
    };
    let mut algs = [
        ED25519_NAME.to_string(),
        P256_NAME.to_string(),
        extra.to_string(),
    ];
    algs.shuffle(rng);
    algs
}

pub fn short_alg(algorithm: &str) -> &'static str {
    match algorithm {
        ED25519_NAME => "Ed25519",
        P256_NAME => "P-256",
        _ => "unknown",
    }
}

pub fn trunc_hex(hex: &str) -> String {
    if hex.len() <= 12 {
        return hex.to_string();
    }
    format!("{}…{}", &hex[..8], &hex[hex.len() - 4..])
}

pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut pending_hyphen = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            pending_hyphen = false;
        } else if !out.is_empty() && !pending_hyphen {
            out.push('-');
            pending_hyphen = true;
        }
    }
    if out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn has_both_algs(algs: &[String; 3]) -> bool {
        algs.iter().any(|alg| alg == ED25519_NAME) && algs.iter().any(|alg| alg == P256_NAME)
    }

    #[test]
    fn preset_algs_always_include_both() {
        for seed in 0..64_u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let algs = assign_preset_algs(&mut rng);
            assert!(has_both_algs(&algs), "seed {seed} => {algs:?}");
        }
    }

    #[test]
    fn seed_creates_three_unique_presets() {
        let roster = Roster::seed().expect("seed");
        assert_eq!(roster.list().len(), 3);
        for id in PRESET_IDS {
            let identity = roster.get(id).expect(id);
            assert!(identity.preset);
            assert_eq!(identity.id, id);
            assert!(!identity.private_hex.is_empty());
            assert!(!identity.public_hex.is_empty());
        }
        let keys: Vec<_> = roster
            .list()
            .iter()
            .map(|identity| identity.private_hex.as_str())
            .collect();
        assert_ne!(keys[0], keys[1]);
        assert_ne!(keys[1], keys[2]);
        assert_ne!(keys[0], keys[2]);
        let algs = [
            roster.get("alice").unwrap().algorithm.clone(),
            roster.get("bob").unwrap().algorithm.clone(),
            roster.get("carol").unwrap().algorithm.clone(),
        ];
        assert!(has_both_algs(&algs));
    }

    #[test]
    fn add_remint_delete_user_identity() {
        let mut roster = Roster::seed().expect("seed");
        let id = roster.add("Dave", ED25519_NAME).expect("add");
        assert_eq!(id, "dave");
        let first = roster.get("dave").expect("dave").clone();
        assert!(!first.preset);
        assert_eq!(first.label, "Dave");

        roster.remint("dave", P256_NAME).expect("remint");
        let second = roster.get("dave").expect("dave");
        assert_eq!(second.algorithm, P256_NAME);
        assert_ne!(second.private_hex, first.private_hex);

        roster.update_keys("dave", "aa", "bb").expect("update");
        assert_eq!(roster.get("dave").unwrap().public_hex, "bb");

        roster.delete("dave").expect("delete");
        assert!(roster.get("dave").is_none());
    }

    #[test]
    fn presets_are_frozen() {
        let mut roster = Roster::seed().expect("seed");
        assert!(roster.remint("alice", ED25519_NAME).is_err());
        assert!(roster.delete("bob").is_err());
        assert!(roster.update_keys("carol", "aa", "bb").is_err());
        assert!(roster.add("Alice", ED25519_NAME).is_err());
        assert!(roster.add("ALICE", ED25519_NAME).is_err());
        assert!(roster.add("   ", ED25519_NAME).is_err());
        assert!(roster.add("!!!", ED25519_NAME).is_err());
    }

    #[test]
    fn names_are_unique_ignoring_case() {
        let mut roster = Roster::seed().expect("seed");
        roster.add("Dave", ED25519_NAME).expect("add");
        assert!(roster.add("dave", P256_NAME).is_err());
        assert!(roster.add("DAVE", P256_NAME).is_err());
        assert!(roster.add("Dave", ED25519_NAME).is_err());
    }

    #[test]
    fn add_verify_only_has_no_private_key() {
        let mut roster = Roster::seed().expect("seed");
        let public = roster.get("alice").unwrap().public_hex.clone();
        let alg = roster.get("alice").unwrap().algorithm.clone();
        let id = roster
            .add_verify_only("Mallory", &alg, &public)
            .expect("add verify-only");
        assert_eq!(id, "mallory");
        let mallory = roster.get("mallory").expect("mallory");
        assert!(mallory.verify_only);
        assert!(mallory.private_hex.is_empty());
        assert_eq!(mallory.public_hex, public);
        assert_eq!(mallory.display_name(), "Mallory (verify only)");
        assert!(roster.remint("mallory", ED25519_NAME).is_err());
        roster.delete("mallory").expect("delete verify-only");
        assert!(roster.get("mallory").is_none());
        assert!(roster.add_verify_only("Eve", ED25519_NAME, "").is_err());
        assert!(
            roster
                .add_verify_only("Eve", ED25519_NAME, "not-a-key")
                .is_err()
        );
        assert!(roster.add_verify_only("Eve", ED25519_NAME, "00").is_err());

        let minted = roster
            .mint_verify_only("Trent", ED25519_NAME)
            .expect("mint verify-only");
        assert_eq!(minted, "trent");
        let trent = roster.get("trent").expect("trent");
        assert!(trent.verify_only);
        assert!(trent.private_hex.is_empty());
        assert!(!trent.public_hex.is_empty());
        crate::keys::canonical_public_key(ED25519_NAME, &trent.public_hex).expect("conformant");
    }

    #[test]
    fn slugify_names() {
        assert_eq!(slugify("Dave Smith"), "dave-smith");
        assert_eq!(slugify("  Carol  "), "carol");
        assert_eq!(slugify("!!!"), "");
        assert_eq!(trunc_hex("aabbccddeeff0011"), "aabbccdd…0011");
        assert_eq!(short_alg(ED25519_NAME), "Ed25519");
        assert_eq!(short_alg(P256_NAME), "P-256");
    }
}
