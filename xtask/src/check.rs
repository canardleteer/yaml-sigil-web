//! Canonically ordered repository checks.

use std::path::Path;

use anyhow::{Result, ensure};

use crate::process::{CommandSpec, require, run as run_command};
use crate::{CheckStep, FeatureArgs, policy};

const RUSTFMT_INSTALL: &str = "Install it with `rustup component add rustfmt`.";
const CLIPPY_INSTALL: &str = "Install it with `rustup component add clippy`.";
const WASM_TARGET_INSTALL: &str = "Install it with `rustup target add wasm32-unknown-unknown`.";

pub fn select_steps(only: &[CheckStep], exclude: &[CheckStep]) -> Result<Vec<CheckStep>> {
    let selected: Vec<_> = policy::CHECK_ORDER
        .iter()
        .copied()
        .filter(|step| {
            if only.is_empty() {
                !exclude.contains(step)
            } else {
                only.contains(step)
            }
        })
        .collect();
    ensure!(
        !selected.is_empty(),
        "the check selection is empty; choose at least one registered step"
    );
    Ok(selected)
}

pub fn run(root: &Path, steps: &[CheckStep], features: &FeatureArgs) -> Result<()> {
    require(
        root,
        "Cargo",
        &CommandSpec::new("cargo").arg("--version"),
        "Install a Rust toolchain from https://rustup.rs/.",
    )?;
    if steps.contains(&CheckStep::Fmt) {
        require(
            root,
            "rustfmt",
            &CommandSpec::new("cargo").args(["fmt", "--version"]),
            RUSTFMT_INSTALL,
        )?;
    }
    if steps
        .iter()
        .any(|step| matches!(step, CheckStep::Clippy | CheckStep::Wasm))
    {
        require(
            root,
            "Clippy",
            &CommandSpec::new("cargo").args(["clippy", "--version"]),
            CLIPPY_INSTALL,
        )?;
    }
    if steps.contains(&CheckStep::Wasm) {
        require(
            root,
            "wasm32-unknown-unknown",
            &CommandSpec::new("rustc").args(["--print", "cfg", "--target", policy::WASM_TARGET]),
            WASM_TARGET_INSTALL,
        )?;
    }

    for step in steps {
        eprintln!("==> {step:?}");
        let command = command_for(*step, features);
        run_command(root, &command)?;
    }
    Ok(())
}

fn command_for(step: CheckStep, features: &FeatureArgs) -> CommandSpec {
    match step {
        CheckStep::Fmt => CommandSpec::new("cargo").args(["fmt", "--all", "--", "--check"]),
        CheckStep::Check => cargo_quality("check", features, false),
        CheckStep::Clippy => cargo_quality("clippy", features, true),
        CheckStep::Test => cargo_quality("test", features, false),
        CheckStep::Wasm => {
            let mut command = CommandSpec::new("cargo").args([
                "clippy",
                "--locked",
                "--package",
                policy::QUALITY_PACKAGE,
                "--target",
                policy::WASM_TARGET,
            ]);
            command.args.extend(features.cargo_args());
            command
                .args
                .extend(["--".into(), "-D".into(), "warnings".into()]);
            command
        }
    }
}

fn cargo_quality(subcommand: &str, features: &FeatureArgs, deny_warnings: bool) -> CommandSpec {
    let mut command =
        CommandSpec::new("cargo").args([subcommand, "--locked", "--workspace", "--all-targets"]);
    command.args.extend(features.cargo_args());
    if deny_warnings {
        command
            .args
            .extend(["--".into(), "-D".into(), "warnings".into()]);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_is_deduplicated_and_canonical() {
        let selected = select_steps(&[CheckStep::Test, CheckStep::Fmt, CheckStep::Test], &[])
            .expect("selection should work");
        assert_eq!(selected, vec![CheckStep::Fmt, CheckStep::Test]);
    }

    #[test]
    fn exclusion_preserves_registry_order() {
        let selected = select_steps(&[], &[CheckStep::Clippy]).expect("selection should work");
        assert_eq!(
            selected,
            vec![
                CheckStep::Fmt,
                CheckStep::Check,
                CheckStep::Test,
                CheckStep::Wasm
            ]
        );
    }

    #[test]
    fn excluding_every_step_is_an_error() {
        assert!(select_steps(&[], &policy::CHECK_ORDER).is_err());
    }

    #[test]
    fn fmt_does_not_receive_feature_arguments() {
        let command = command_for(CheckStep::Fmt, &FeatureArgs::default());
        assert!(!command.args.contains(&"--all-features".into()));
    }

    #[test]
    fn wasm_targets_the_playground_package() {
        let command = command_for(CheckStep::Wasm, &FeatureArgs::default());
        assert!(command.args.contains(&"--target".into()));
        assert!(command.args.contains(&policy::WASM_TARGET.into()));
        assert!(command.args.contains(&policy::QUALITY_PACKAGE.into()));
        assert!(command.args.contains(&"-D".into()));
    }
}
