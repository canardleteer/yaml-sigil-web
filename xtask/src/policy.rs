//! Repository-owned seams for checks, coverage, images, and Trunk.

use crate::{CheckStep, ImageEngine};

pub const CHECK_ORDER: [CheckStep; 5] = [
    CheckStep::Fmt,
    CheckStep::Check,
    CheckStep::Clippy,
    CheckStep::Test,
    CheckStep::Wasm,
];

pub const QUALITY_PACKAGE: &str = "yaml-sigil-web";
pub const WASM_TARGET: &str = "wasm32-unknown-unknown";

pub const IMAGE_AUTO_ORDER: [ImageEngine; 2] = [ImageEngine::Docker, ImageEngine::Buildah];
pub const IMAGE_FILE: &str = "Dockerfile";
pub const IMAGE_CONTEXT: &str = ".";
pub const IMAGE_TAG: &str = "yaml-sigil-web:local";
pub const IMAGE_DIST_PATH: &str = "/app/dist/index.html";

pub const LLVM_COV_REPORT: &str = "target/coverage/llvm-cov/html/index.html";
pub const TARPAULIN_REPORT: &str = "target/coverage/tarpaulin/tarpaulin-report.html";
pub const COVERAGE_IGNORE_REGEX: &str = "src/(app|lib)\\.rs";
pub const COVERAGE_FAIL_UNDER_LINES: u32 = 90;
