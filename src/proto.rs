//! Typed protobuf views for playground forms (`SignedYamlArtifact`, `YamlSigilSignature`).

use buffa::{Enumeration, Message};
use yaml_sigil_core::pb::{Algorithm, YamlSigilSignature};
use yaml_sigil_core::{
    decode_signature_carrier, decode_signed_yaml_artifact, view_signed_yaml_artifact,
};

use crate::codec::{decode_binary_field, decode_payload, encode_payload, encode_yaml_text};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureFields {
    pub alg: String,
    pub keyid: String,
    pub signature_b64: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactFields {
    pub payload: String,
    pub signature: SignatureFields,
}

pub fn parse_artifact_text(input: &str) -> Result<ArtifactFields, String> {
    let bytes = decode_payload(input, "protobuf")?;
    if bytes.is_empty() {
        return Err("empty protobuf".into());
    }
    let msg = decode_signed_yaml_artifact(&bytes).map_err(|error| error.to_string())?;
    let view = view_signed_yaml_artifact(&msg).map_err(|error| error.to_string())?;
    Ok(ArtifactFields {
        payload: encode_yaml_text(&view.payload),
        signature: signature_fields(view.alg_wire, view.keyid.as_deref(), &view.signature),
    })
}

pub fn parse_carrier_text(input: &str) -> Result<SignatureFields, String> {
    let bytes = decode_payload(input, "protobuf")?;
    if bytes.is_empty() {
        return Err("empty protobuf".into());
    }
    let sig = decode_signature_carrier(&bytes).map_err(|error| error.to_string())?;
    Ok(signature_fields(
        sig.alg.to_i32(),
        sig.keyid.as_deref(),
        &sig.signature,
    ))
}

pub fn encode_carrier_text(
    alg_name: &str,
    keyid: &str,
    signature_b64: &str,
) -> Result<String, String> {
    let alg = Algorithm::from_proto_name(alg_name).ok_or("invalid algorithm")?;
    let signature = decode_binary_field(signature_b64)?;
    let keyid = {
        let trimmed = keyid.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };
    let bytes = YamlSigilSignature {
        alg: alg.into(),
        keyid,
        signature,
        ..Default::default()
    }
    .encode_to_vec();
    Ok(encode_payload(&bytes, "protobuf"))
}

fn signature_fields(alg_wire: i32, keyid: Option<&str>, signature: &[u8]) -> SignatureFields {
    let alg = Algorithm::from_i32(alg_wire)
        .map(|alg| alg.proto_name().to_string())
        .unwrap_or_else(|| format!("unknown ({alg_wire})"));
    SignatureFields {
        alg,
        keyid: keyid.unwrap_or_default().to_string(),
        signature_b64: encode_payload(signature, "protobuf"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::generate_keypair;
    use crate::ops::{self, ED25519_NAME};

    const YAML: &str = "claim: ridge-line cache\nseason: 2026\n";

    #[test]
    fn protobuf_artifact_and_carrier_round_trip() {
        let pair = generate_keypair(ED25519_NAME).expect("keys");
        let signed = ops::sign(
            YAML,
            ED25519_NAME,
            &pair.private_hex,
            Some("demo"),
            true,
            "protobuf",
        );
        assert_eq!(signed.status, "success", "{signed:?}");

        let artifact = parse_artifact_text(&signed.primary).expect("artifact");
        assert_eq!(artifact.payload, YAML);
        assert_eq!(
            artifact.signature.alg,
            "ALGORITHM_ED25519_PUREEDDSA_RAW_RS64_CANONICAL"
        );
        assert_eq!(artifact.signature.keyid, "demo");
        assert!(!artifact.signature.signature_b64.is_empty());

        let parts = ops::decompose(&signed.primary, "protobuf", Some("strict"));
        assert_eq!(parts.status, "ok", "{parts:?}");
        let carrier = parse_carrier_text(&parts.extra).expect("carrier");
        assert_eq!(carrier, artifact.signature);

        let reencoded = encode_carrier_text(&carrier.alg, &carrier.keyid, &carrier.signature_b64)
            .expect("encode");
        let rebuilt = ops::compose(&parts.primary, &reencoded, "protobuf");
        assert_eq!(rebuilt.status, "success", "{rebuilt:?}");
        assert_eq!(rebuilt.primary, signed.primary);
    }

    #[test]
    fn garbage_protobuf_is_an_error() {
        assert!(parse_artifact_text("$$$$").is_err());
        assert!(parse_carrier_text("$$$$").is_err());
        assert!(parse_artifact_text("").is_err());
        assert!(encode_carrier_text("nope", "", "").is_err());
    }
}
