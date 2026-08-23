//! QR encoding of YAML text for the playground (SVG + module matrix).

use qrcode::QrCode;
use qrcode::render::svg;

#[derive(Clone, Debug)]
pub struct QrImage {
    pub svg: String,
    /// Row-major, `true` is a dark module.
    pub modules: Vec<bool>,
    pub width: usize,
}

pub fn encode(text: &str) -> Result<QrImage, String> {
    if text.is_empty() {
        return Err("empty".into());
    }
    let code = QrCode::new(text.as_bytes()).map_err(|error| error.to_string())?;
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
    !text.is_empty() && QrCode::new(text.as_bytes()).is_ok()
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
