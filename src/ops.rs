//! YamlSigil compose / decompose / sign / verify plus noyalib validation.

use ed25519_dalek::SigningKey as Ed25519SigningKey;
use p256::ecdsa::SigningKey as P256SigningKey;
use yaml_sigil_core::AlgorithmId;
use yaml_sigil_signing::{
    OutputForm, SignError, SignInvocationError, SignOutcome, SignRequest, SigningKey,
    TranscodeError, proto_wire_to_signed_yaml_stream, sign as sign_runtime,
    signed_yaml_stream_to_proto_wire,
};
use yaml_sigil_transcription::{
    ComposeOutcome, ComposeRequest, DecomposeOutcome, DecomposeRequest, DecomposeResponse,
    OuterConformance, TranscriberError, TranscriberInvocationError, TranscriptionForm,
    compose as compose_runtime, decompose as decompose_runtime,
};
use yaml_sigil_verification::{
    ArtifactForm, InvocationError, PublicKeys, VerifierOptions, VerifierState,
    resolve_ed25519_verifying_key, resolve_p256_verifying_key, verify as verify_runtime,
};
use zeroize::{Zeroize, Zeroizing};

use crate::codec::{
    decode_key_bytes, decode_payload, decode_yaml_text, encode_payload, encode_yaml_text,
};

pub const ED25519_NAME: &str = "ED25519_PUREEDDSA_RAW_RS64_CANONICAL";
pub const P256_NAME: &str = "ECDSA_SECP256R1_SHA256_RAW_RS64";

#[derive(Clone, Debug)]
pub struct OpResult {
    pub status: String,
    pub code: Option<String>,
    pub primary: String,
    pub extra: String,
    pub extra_label: String,
}

impl OpResult {
    fn with_status(status: &str) -> Self {
        Self {
            status: status.to_string(),
            code: None,
            primary: String::new(),
            extra: String::new(),
            extra_label: String::new(),
        }
    }

    fn invocation(code: &str) -> Self {
        Self {
            status: "invocation_error".into(),
            code: Some(code.into()),
            primary: String::new(),
            extra: String::new(),
            extra_label: String::new(),
        }
    }

    pub fn invocation_error(code: &str) -> Self {
        Self::invocation(code)
    }

    fn err(status: &str, code: &str) -> Self {
        Self {
            status: status.into(),
            code: Some(code.into()),
            primary: String::new(),
            extra: String::new(),
            extra_label: String::new(),
        }
    }
}

pub fn compose(payload_text: &str, carrier_text: &str, form: &str) -> OpResult {
    let Some(form_sel) = transcription_form(form) else {
        return OpResult::invocation("invalid_or_unsupported_form");
    };
    let payload = decode_yaml_text(payload_text);
    let signature_carrier = match decode_payload(carrier_text, form) {
        Ok(bytes) => bytes,
        Err(msg) => return OpResult::err("invocation_error", &msg),
    };
    match compose_runtime(&ComposeRequest {
        payload: &payload,
        signature_carrier: &signature_carrier,
        form: form_sel,
    }) {
        ComposeOutcome::Success(success) => {
            let mut result = OpResult::with_status("success");
            result.primary = encode_payload(&success.artifact, form);
            result
        }
        ComposeOutcome::Invocation(error) => {
            OpResult::invocation(transcriber_invocation_code(error))
        }
        ComposeOutcome::Error(error) => OpResult::err("error", transcriber_error_code(error)),
    }
}

pub fn decompose(artifact_text: &str, form: &str, outer: Option<&str>) -> OpResult {
    let Some(form_sel) = transcription_form(form) else {
        return OpResult::invocation("invalid_or_unsupported_form");
    };
    let outer_conformance = match (form, outer.map(str::trim).filter(|s| !s.is_empty())) {
        ("yaml", Some(_)) => {
            return OpResult::invocation("invalid_or_unsupported_outer_conformance");
        }
        ("yaml", None) => None,
        ("protobuf", None) => {
            return OpResult::invocation("invalid_or_unsupported_outer_conformance");
        }
        ("protobuf", Some(value)) => match outer_conformance(value) {
            Some(parsed) => Some(parsed),
            None => return OpResult::invocation("invalid_or_unsupported_outer_conformance"),
        },
        _ => return OpResult::invocation("invalid_or_unsupported_form"),
    };
    let artifact = match decode_payload(artifact_text, form) {
        Ok(bytes) => bytes,
        Err(msg) => return OpResult::err("invocation_error", &msg),
    };
    match decompose_runtime(&DecomposeRequest {
        artifact: &artifact,
        form: form_sel,
        outer_conformance,
    }) {
        DecomposeResponse::Invocation(error) => {
            OpResult::invocation(transcriber_invocation_code(error))
        }
        DecomposeResponse::Structural(result) => match result.outcome {
            DecomposeOutcome::Ok => {
                let mut out = OpResult::with_status("ok");
                if let Some(payload) = result.payload {
                    out.primary = encode_yaml_text(&payload);
                }
                if let Some(carrier) = result.signature_carrier {
                    out.extra = encode_payload(&carrier, form);
                    out.extra_label = "signature carrier".into();
                }
                out
            }
            DecomposeOutcome::Unsigned => yaml_unsigned_or_not_document(form, &artifact),
            DecomposeOutcome::MalformedAttemptedSigned => {
                OpResult::with_status("malformed_attempted_signed")
            }
        },
    }
}

pub fn transcode(artifact_text: &str, from_form: &str, to_form: &str) -> OpResult {
    if transcription_form(from_form).is_none() || transcription_form(to_form).is_none() {
        return OpResult::invocation("invalid_or_unsupported_form");
    }
    if from_form == to_form {
        let mut out = OpResult::with_status("success");
        out.primary = artifact_text.to_string();
        return out;
    }
    let decoded = match decode_payload(artifact_text, from_form) {
        Ok(bytes) => bytes,
        Err(msg) => return OpResult::err("invocation_error", &msg),
    };
    let converted = match (from_form, to_form) {
        ("yaml", "protobuf") => signed_yaml_stream_to_proto_wire(&decoded),
        ("protobuf", "yaml") => proto_wire_to_signed_yaml_stream(&decoded),
        _ => return OpResult::invocation("invalid_or_unsupported_form"),
    };
    match converted {
        Ok(bytes) => {
            let mut out = OpResult::with_status("success");
            out.primary = encode_payload(&bytes, to_form);
            out
        }
        Err(error) => OpResult::err("transcode_error", transcode_error_code(&error)),
    }
}

pub fn sign(
    payload_text: &str,
    algorithm_selector: &str,
    signing_key: &str,
    keyid: Option<&str>,
    append_missing_final_newline: bool,
    output_form_selector: &str,
) -> OpResult {
    let Some(algorithm) = algorithm(algorithm_selector) else {
        return OpResult::invocation("invalid_or_unsupported_algorithm");
    };
    let Some(output_form) = output_form(output_form_selector) else {
        return OpResult::invocation("invalid_or_unsupported_output_form");
    };
    let payload = decode_yaml_text(payload_text);
    let mut key_bytes = match decode_key_bytes(signing_key) {
        Ok(bytes) => Zeroizing::new(bytes),
        Err(msg) => return OpResult::err("invocation_error", &msg),
    };
    let result = match algorithm {
        AlgorithmId::Ed25519 => {
            let Ok(seed) = <&[u8; 32]>::try_from(key_bytes.as_slice()) else {
                return OpResult::invocation("invalid_signing_key");
            };
            let key = Ed25519SigningKey::from_bytes(seed);
            sign_with_key(
                &payload,
                algorithm,
                SigningKey::Ed25519(&key),
                keyid.filter(|s| !s.is_empty()),
                append_missing_final_newline,
                output_form,
                output_form_selector,
            )
        }
        AlgorithmId::EcdsaP256Sha256 => {
            let Ok(key) = P256SigningKey::from_slice(&key_bytes) else {
                return OpResult::invocation("invalid_signing_key");
            };
            sign_with_key(
                &payload,
                algorithm,
                SigningKey::EcdsaP256Sha256(&key),
                keyid.filter(|s| !s.is_empty()),
                append_missing_final_newline,
                output_form,
                output_form_selector,
            )
        }
    };
    key_bytes.zeroize();
    result
}

fn sign_with_key(
    payload: &[u8],
    algorithm: AlgorithmId,
    key: SigningKey<'_>,
    keyid: Option<&str>,
    append_missing_final_newline: bool,
    output_form: OutputForm,
    form_label: &str,
) -> OpResult {
    match sign_runtime(&SignRequest {
        payload,
        algorithm,
        key,
        keyid,
        append_missing_final_newline,
        output_form,
        algorithm_parameters: &[],
    }) {
        SignOutcome::Success(success) => {
            let mut result = OpResult::with_status("success");
            result.primary = encode_payload(&success.artifact, form_label);
            if !success.modified_payload.is_empty() {
                result.extra = encode_yaml_text(&success.modified_payload);
                result.extra_label = "modified payload".into();
            }
            result
        }
        SignOutcome::Invocation(error) => OpResult::invocation(sign_invocation_code(error)),
        SignOutcome::Signer(error) => OpResult::err("signer_error", sign_error_code(&error)),
    }
}

pub fn verify(
    artifact_text: &str,
    form_selector: &str,
    algorithm_selector: &str,
    verifying_key: &str,
) -> OpResult {
    let Some(form) = artifact_form(form_selector) else {
        return OpResult::invocation("invalid_or_unsupported_form");
    };
    let Some(selected_algorithm) = algorithm(algorithm_selector) else {
        return OpResult::invocation("invalid_or_unsupported_algorithm");
    };
    let artifact = match decode_payload(artifact_text, form_selector) {
        Ok(bytes) => bytes,
        Err(msg) => return OpResult::err("invocation_error", &msg),
    };
    let key_bytes = match decode_key_bytes(verifying_key) {
        Ok(bytes) => bytes,
        Err(msg) => return OpResult::err("invocation_error", &msg),
    };
    let ed_key;
    let p256_key;
    let keys = match selected_algorithm {
        AlgorithmId::Ed25519 => match resolve_ed25519_verifying_key(&key_bytes) {
            Ok(key) => {
                ed_key = key;
                PublicKeys {
                    ed25519: Some(&ed_key),
                    p256: None,
                }
            }
            Err(error) => return OpResult::invocation(verify_invocation_code(error)),
        },
        AlgorithmId::EcdsaP256Sha256 => match resolve_p256_verifying_key(&key_bytes) {
            Ok(key) => {
                p256_key = key;
                PublicKeys {
                    ed25519: None,
                    p256: Some(&p256_key),
                }
            }
            Err(error) => return OpResult::invocation(verify_invocation_code(error)),
        },
    };
    let options = VerifierOptions {
        verify_ed25519: selected_algorithm == AlgorithmId::Ed25519,
        verify_ecdsa_p256_sha256: selected_algorithm == AlgorithmId::EcdsaP256Sha256,
        ..VerifierOptions::default()
    };
    match verify_runtime(&artifact, form, &keys, options) {
        Err(error) => OpResult::invocation(verify_invocation_code(error)),
        Ok(VerifierState::Verified { payload, algorithm }) => {
            let mut result = OpResult::with_status("verified");
            result.primary = encode_yaml_text(&payload);
            result.extra = algorithm_name(algorithm).to_string();
            result.extra_label = "algorithm".into();
            result
        }
        Ok(VerifierState::Unsigned) => OpResult::with_status("unsigned"),
        Ok(VerifierState::MalformedAttemptedSigned) => {
            OpResult::with_status("malformed_attempted_signed")
        }
        Ok(VerifierState::SignedButAlgorithmUnsupported { algorithm }) => {
            let mut result = OpResult::with_status("signed_but_algorithm_unsupported");
            result.extra = algorithm_name(algorithm).to_string();
            result.extra_label = "algorithm".into();
            result
        }
        Ok(VerifierState::SignedButFailedVerification) => {
            OpResult::with_status("signed_but_failed_verification")
        }
    }
}

pub fn validate_yaml(source: &str) -> OpResult {
    match noyalib::from_str::<serde_json::Value>(source) {
        Ok(_) => OpResult::with_status("success"),
        Err(error) => OpResult::err("error", &error.to_string()),
    }
}

fn yaml_unsigned_or_not_document(form: &str, artifact: &[u8]) -> OpResult {
    if form != "yaml" {
        return OpResult::with_status("unsigned");
    }
    let Ok(text) = std::str::from_utf8(artifact) else {
        return OpResult::err("could_not_decompose", "not_a_yaml_document");
    };
    match noyalib::from_str::<serde_json::Value>(text) {
        Ok(_) => OpResult::with_status("unsigned"),
        Err(_) => OpResult::err("could_not_decompose", "not_a_yaml_document"),
    }
}

fn transcription_form(value: &str) -> Option<TranscriptionForm> {
    match value {
        "yaml" => Some(TranscriptionForm::Yaml),
        "protobuf" => Some(TranscriptionForm::Protobuf),
        _ => None,
    }
}

fn output_form(value: &str) -> Option<OutputForm> {
    match value {
        "yaml" => Some(OutputForm::Yaml),
        "protobuf" => Some(OutputForm::Protobuf),
        _ => None,
    }
}

fn artifact_form(value: &str) -> Option<ArtifactForm> {
    match value {
        "yaml" => Some(ArtifactForm::Yaml),
        "protobuf" => Some(ArtifactForm::Proto),
        _ => None,
    }
}

fn outer_conformance(value: &str) -> Option<OuterConformance> {
    match value {
        "strict" => Some(OuterConformance::Strict),
        "signature_strict" => Some(OuterConformance::SignatureStrict),
        _ => None,
    }
}

fn algorithm(value: &str) -> Option<AlgorithmId> {
    match value {
        ED25519_NAME => Some(AlgorithmId::Ed25519),
        P256_NAME => Some(AlgorithmId::EcdsaP256Sha256),
        _ => None,
    }
}

fn algorithm_name(value: AlgorithmId) -> &'static str {
    match value {
        AlgorithmId::Ed25519 => ED25519_NAME,
        AlgorithmId::EcdsaP256Sha256 => P256_NAME,
    }
}

fn transcriber_invocation_code(error: TranscriberInvocationError) -> &'static str {
    match error {
        TranscriberInvocationError::InvalidOrUnsupportedForm => "invalid_or_unsupported_form",
        TranscriberInvocationError::InvalidOrUnsupportedOuterConformance => {
            "invalid_or_unsupported_outer_conformance"
        }
    }
}

fn transcriber_error_code(error: TranscriberError) -> &'static str {
    match error {
        TranscriberError::InvalidPayloadBytes => "invalid_payload_bytes",
        TranscriberError::InvalidSignatureCarrier => "invalid_signature_carrier",
    }
}

fn transcode_error_code(error: &TranscodeError) -> &'static str {
    match error {
        TranscodeError::NotSignedYamlStream => "not_signed_yaml_stream",
        TranscodeError::PayloadInvariant => "payload_invariant",
        TranscodeError::InvalidSignatureBase64 => "invalid_signature_base64",
        TranscodeError::UnknownYamlAlg => "unknown_yaml_alg",
        TranscodeError::UnsupportedWireAlg => "unsupported_wire_alg",
        TranscodeError::SchemaMismatch => "schema_mismatch",
        TranscodeError::Core(_) => "core_error",
        TranscodeError::YamlSerialize(_) => "yaml_serialize",
    }
}

fn sign_invocation_code(error: SignInvocationError) -> &'static str {
    match error {
        SignInvocationError::InvalidOrUnsupportedAlgorithm => "invalid_or_unsupported_algorithm",
        SignInvocationError::InvalidAlgorithmParameters => "invalid_algorithm_parameters",
        SignInvocationError::InvalidOrUnsupportedOutputForm => "invalid_or_unsupported_output_form",
        SignInvocationError::InvalidKeyid => "invalid_keyid",
    }
}

fn sign_error_code(error: &SignError) -> &'static str {
    match error {
        SignError::InvalidPayloadBytes => "invalid_payload_bytes",
        SignError::PayloadLineTerminatorRefusal => "payload_line_terminator_refusal",
        SignError::InvalidOrUnsupportedAlgorithm => "invalid_or_unsupported_algorithm",
        SignError::InvalidAlgorithmParameters => "invalid_algorithm_parameters",
        SignError::InvalidOrUnsupportedOutputForm => "invalid_or_unsupported_output_form",
        SignError::InvalidKeyid => "invalid_keyid",
        SignError::KeyOperationFailure => "key_operation_failure",
        SignError::YamlValidationFailure => "yaml_validation_failure",
        SignError::YamlSerialize(_) => "yaml_serialize",
    }
}

fn verify_invocation_code(error: InvocationError) -> &'static str {
    match error {
        InvocationError::InvalidAlgorithmParameters => "invalid_algorithm_parameters",
        InvocationError::KeyResolutionFailure => "key_resolution_failure",
        InvocationError::TrustPolicyConfigurationError => "trust_policy_configuration_error",
        InvocationError::InvalidPreVerifyResult => "invalid_pre_verify_result",
        InvocationError::InvalidOrUnsupportedForm => "invalid_or_unsupported_form",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{decode_key_bytes, to_hex};
    use crate::keys::generate_keypair;

    const YAML: &str = "claim: ridge-line cache\nseason: 2026\n";

    #[test]
    fn validate_yaml_parse() {
        let parsed = validate_yaml(YAML);
        assert_eq!(parsed.status, "success");

        let bad = validate_yaml("foo: [unterminated");
        assert_eq!(bad.status, "error", "{bad:?}");
    }

    #[test]
    fn sign_verify_compose_decompose_yaml_round_trip() {
        let pair = generate_keypair(ED25519_NAME).expect("keys");
        let signed = sign(
            YAML,
            ED25519_NAME,
            &pair.private_hex,
            Some("demo"),
            true,
            "yaml",
        );
        assert_eq!(signed.status, "success", "{signed:?}");
        assert!(!signed.primary.is_empty());

        let verified = verify(&signed.primary, "yaml", ED25519_NAME, &pair.public_hex);
        assert_eq!(verified.status, "verified", "{verified:?}");
        assert_eq!(verified.primary, YAML);

        let parts = decompose(&signed.primary, "yaml", None);
        assert_eq!(parts.status, "ok", "{parts:?}");
        assert_eq!(parts.primary, YAML);
        assert!(!parts.extra.is_empty());

        let rebuilt = compose(&parts.primary, &parts.extra, "yaml");
        assert_eq!(rebuilt.status, "success", "{rebuilt:?}");
        assert_eq!(rebuilt.primary, signed.primary);
    }

    #[test]
    fn sign_verify_p256_yaml_round_trip() {
        let pair = generate_keypair(P256_NAME).expect("keys");
        let signed = sign(YAML, P256_NAME, &pair.private_hex, None, true, "yaml");
        assert_eq!(signed.status, "success", "{signed:?}");

        let verified = verify(&signed.primary, "yaml", P256_NAME, &pair.public_hex);
        assert_eq!(verified.status, "verified", "{verified:?}");
        assert_eq!(verified.primary, YAML);

        let bytes = decode_key_bytes(&pair.public_hex).expect("hex");
        let key = resolve_p256_verifying_key(&bytes).expect("uncompressed");
        let compressed = to_hex(key.to_encoded_point(true).as_bytes());
        let rejected = verify(&signed.primary, "yaml", P256_NAME, &compressed);
        assert_eq!(rejected.status, "invocation_error", "{rejected:?}");
        assert_eq!(rejected.code.as_deref(), Some("key_resolution_failure"));
    }

    #[test]
    fn protobuf_compose_preserves_payload_without_yaml_stream_rules() {
        let pair = generate_keypair(ED25519_NAME).expect("keys");
        let signed = sign(
            YAML,
            ED25519_NAME,
            &pair.private_hex,
            None,
            true,
            "protobuf",
        );
        assert_eq!(signed.status, "success", "{signed:?}");
        let parts = decompose(&signed.primary, "protobuf", Some("strict"));
        assert_eq!(parts.status, "ok", "{parts:?}");

        let no_nl = "claim: ridge-line cache\nseason: 2026";
        let proto = compose(no_nl, &parts.extra, "protobuf");
        assert_eq!(proto.status, "success", "{proto:?}");

        let yaml = compose(no_nl, "carrier\n", "yaml");
        assert_eq!(yaml.status, "error", "{yaml:?}");
        assert_eq!(yaml.code.as_deref(), Some("invalid_payload_bytes"));
    }

    #[test]
    fn sign_verify_compose_decompose_protobuf_round_trip() {
        let pair = generate_keypair(ED25519_NAME).expect("keys");
        let signed = sign(
            YAML,
            ED25519_NAME,
            &pair.private_hex,
            Some("demo"),
            true,
            "protobuf",
        );
        assert_eq!(signed.status, "success", "{signed:?}");
        assert!(!signed.primary.is_empty());
        assert_ne!(signed.primary, YAML);

        let verified = verify(&signed.primary, "protobuf", ED25519_NAME, &pair.public_hex);
        assert_eq!(verified.status, "verified", "{verified:?}");
        assert_eq!(verified.primary, YAML);

        let parts = decompose(&signed.primary, "protobuf", Some("strict"));
        assert_eq!(parts.status, "ok", "{parts:?}");
        assert_eq!(parts.primary, YAML);
        assert!(!parts.extra.is_empty());

        let rebuilt = compose(&parts.primary, &parts.extra, "protobuf");
        assert_eq!(rebuilt.status, "success", "{rebuilt:?}");
        assert_eq!(rebuilt.primary, signed.primary);
    }

    #[test]
    fn selectors_are_exact() {
        let signed = sign(YAML, "ed25519", "00", None, true, "yaml");
        assert_eq!(signed.status, "invocation_error");
        let composed = compose(YAML, YAML, "YAML");
        assert_eq!(composed.status, "invocation_error");
        let yaml_outer = decompose(YAML, "yaml", Some("strict"));
        assert_eq!(yaml_outer.status, "invocation_error");
        let proto_missing_outer = decompose("AAAA", "protobuf", None);
        assert_eq!(proto_missing_outer.status, "invocation_error");
    }

    #[test]
    fn yaml_protobuf_transcode_round_trip_verifies() {
        let pair = generate_keypair(ED25519_NAME).expect("keys");
        let signed = sign(
            YAML,
            ED25519_NAME,
            &pair.private_hex,
            Some("demo"),
            true,
            "yaml",
        );
        assert_eq!(signed.status, "success", "{signed:?}");

        let proto = transcode(&signed.primary, "yaml", "protobuf");
        assert_eq!(proto.status, "success", "{proto:?}");
        assert_ne!(proto.primary, signed.primary);

        let verified_proto = verify(&proto.primary, "protobuf", ED25519_NAME, &pair.public_hex);
        assert_eq!(verified_proto.status, "verified", "{verified_proto:?}");
        assert_eq!(verified_proto.primary, YAML);

        let yaml_again = transcode(&proto.primary, "protobuf", "yaml");
        assert_eq!(yaml_again.status, "success", "{yaml_again:?}");
        let verified_yaml = verify(&yaml_again.primary, "yaml", ED25519_NAME, &pair.public_hex);
        assert_eq!(verified_yaml.status, "verified", "{verified_yaml:?}");
        assert_eq!(verified_yaml.primary, YAML);

        let same = transcode(&signed.primary, "yaml", "yaml");
        assert_eq!(same.status, "success");
        assert_eq!(same.primary, signed.primary);
    }

    #[test]
    fn transcode_rejects_unsigned_yaml() {
        let failed = transcode(YAML, "yaml", "protobuf");
        assert_eq!(failed.status, "transcode_error", "{failed:?}");
        assert_eq!(failed.code.as_deref(), Some("not_signed_yaml_stream"));
    }

    #[test]
    fn yaml_join_of_protobuf_carrier_is_not_signed_yaml() {
        let pair = generate_keypair(ED25519_NAME).expect("keys");
        let signed = sign(
            YAML,
            ED25519_NAME,
            &pair.private_hex,
            None,
            true,
            "protobuf",
        );
        assert_eq!(signed.status, "success", "{signed:?}");
        let parts = decompose(&signed.primary, "protobuf", Some("strict"));
        assert_eq!(parts.status, "ok", "{parts:?}");

        let joined = compose(&parts.primary, &parts.extra, "yaml");
        assert_eq!(joined.status, "success", "{joined:?}");
        let trans = transcode(&joined.primary, "yaml", "protobuf");
        assert_eq!(trans.status, "transcode_error", "{trans:?}");
    }

    #[test]
    fn yaml_decompose_rejects_non_yaml() {
        let junk = decompose("foo: [unterminated", "yaml", None);
        assert_eq!(junk.status, "could_not_decompose", "{junk:?}");
        assert_eq!(junk.code.as_deref(), Some("not_a_yaml_document"));

        let unsigned = decompose(YAML, "yaml", None);
        assert_eq!(unsigned.status, "unsigned", "{unsigned:?}");
    }
}
