//! Trunk serve and static-site build.

use std::ffi::OsString;
use std::path::Path;

use anyhow::Result;

use crate::process::{CommandSpec, require, run as run_command};

const TRUNK_INSTALL: &str = "Install it with `cargo install --locked trunk --version 0.21.14`.";

pub fn serve(root: &Path, release: bool, extra: &[OsString]) -> Result<()> {
    require_trunk(root)?;
    run_command(root, &serve_command(release, extra))
}

pub fn build(root: &Path, debug: bool) -> Result<()> {
    require_trunk(root)?;
    run_command(root, &build_command(debug))
}

fn require_trunk(root: &Path) -> Result<()> {
    require(
        root,
        "Trunk",
        &CommandSpec::new("trunk").arg("--version"),
        TRUNK_INSTALL,
    )
}

pub(crate) fn serve_command(release: bool, extra: &[OsString]) -> CommandSpec {
    let mut command = CommandSpec::new("trunk").arg("serve");
    if release {
        command = command.args(["--release", "true"]);
    }
    command.args.extend(extra.iter().cloned());
    command
}

pub(crate) fn build_command(debug: bool) -> CommandSpec {
    if debug {
        CommandSpec::new("trunk").arg("build")
    } else {
        CommandSpec::new("trunk").args(["build", "--release"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_forwards_release_and_extra_args() {
        let command = serve_command(true, &["--port".into(), "9000".into()]);
        assert_eq!(
            command.args,
            vec![
                OsString::from("serve"),
                OsString::from("--release"),
                OsString::from("true"),
                OsString::from("--port"),
                OsString::from("9000"),
            ]
        );
    }

    #[test]
    fn build_defaults_to_release() {
        assert_eq!(
            build_command(false).args,
            vec![OsString::from("build"), OsString::from("--release")]
        );
        assert_eq!(build_command(true).args, vec![OsString::from("build")]);
    }
}
