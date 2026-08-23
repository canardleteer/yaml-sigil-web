//! QR encoding of YAML text for the playground (SVG + module matrix).

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};

/// Medium error correction: about 15% redundancy.
const EC_LEVEL: EcLevel = EcLevel::M;

fn qr_code(text: &str) -> Result<QrCode, String> {
    if text.is_empty() {
        return Err("empty".into());
    }
    QrCode::with_error_correction_level(text.as_bytes(), EC_LEVEL)
        .map_err(|error| error.to_string())
}

#[derive(Clone, Debug)]
pub struct QrImage {
    pub svg: String,
    /// Row-major, `true` is a dark module.
    pub modules: Vec<bool>,
    pub width: usize,
}

pub fn encode(text: &str) -> Result<QrImage, String> {
    let code = qr_code(text)?;
    let width = code.width();
    let mut modules = Vec::with_capacity(width * width);
    for y in 0..width {
        for x in 0..width {
            modules.push(code[(x, y)] == qrcode::Color::Dark);
        }
    }
    let svg = code
        .render::<svg::Color<'_>>()
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .min_dimensions(512, 512)
        .quiet_zone(true)
        .build();
    Ok(QrImage {
        svg,
        modules,
        width,
    })
}

pub fn can_encode(text: &str) -> bool {
    qr_code(text).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_yaml_encodes() {
        let yaml = "claim: ridge-line cache\nseason: 2026\n---\nalg: ED25519_PUREEDDSA_RAW_RS64_CANONICAL\nkeyid: alice\n";
        let image = encode(yaml).expect("encode");
        assert!(image.svg.contains("<svg"));
        assert!(image.svg.contains("#000000"));
        assert!(image.svg.contains("#ffffff"));
        assert_eq!(image.modules.len(), image.width * image.width);
        assert!(image.width >= 21);
        assert_eq!(
            qr_code(yaml).expect("code").error_correction_level(),
            EcLevel::M
        );
        assert!(can_encode(yaml));
    }

    #[test]
    fn empty_and_oversized_fail() {
        assert!(encode("").is_err());
        assert!(!can_encode(""));
        let huge = "x".repeat(8_000);
        assert!(encode(&huge).is_err());
        assert!(!can_encode(&huge));
    }
}
