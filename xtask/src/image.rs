//! Local OCI image builds with Docker and Buildah.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};

use crate::ImageEngine;
use crate::policy;
use crate::process::{CommandSpec, best_effort, probe, run as run_command};

const DOCKER_GUIDANCE: &str =
    "Follow https://docs.docker.com/engine/install/ and verify the local daemon is reachable.";
const BUILDAH_GUIDANCE: &str = "Follow https://github.com/containers/buildah/blob/main/install.md and verify its storage/runtime setup.";

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImagePlan {
    file: &'static str,
    context: &'static str,
    tag: String,
}

pub(crate) fn run(root: &Path, requested: ImageEngine, tag: String) -> Result<()> {
    let engine = resolve_engine(root, requested)?;
    let plan = plan(tag);
    match engine {
        ImageEngine::Docker => docker(root, &plan)?,
        ImageEngine::Buildah => buildah(root, &plan)?,
        ImageEngine::Auto => unreachable!("auto is resolved before execution"),
    }
    Ok(())
}

fn plan(tag: String) -> ImagePlan {
    ImagePlan {
        file: policy::IMAGE_FILE,
        context: policy::IMAGE_CONTEXT,
        tag,
    }
}

fn resolve_engine(root: &Path, requested: ImageEngine) -> Result<ImageEngine> {
    if requested != ImageEngine::Auto {
        engine_probe(root, requested).map_err(anyhow::Error::msg)?;
        return Ok(requested);
    }

    let mut failures = Vec::new();
    for engine in policy::IMAGE_AUTO_ORDER {
        match engine_probe(root, engine) {
            Ok(()) => return Ok(engine),
            Err(error) => {
                eprintln!("Skipping {engine:?}: {error}");
                failures.push(error);
            }
        }
    }
    bail!(
        "no usable OCI engine in configured auto order:\n- {}",
        failures.join("\n- ")
    )
}

fn engine_probe(root: &Path, engine: ImageEngine) -> Result<(), String> {
    match engine {
        ImageEngine::Docker => probe(
            root,
            "Docker",
            &CommandSpec::new("docker").args(["info", "--format", "{{.ServerVersion}}"]),
            DOCKER_GUIDANCE,
        ),
        ImageEngine::Buildah => probe(
            root,
            "Buildah",
            &CommandSpec::new("buildah").arg("info"),
            BUILDAH_GUIDANCE,
        ),
        ImageEngine::Auto => Err("auto is not an executable engine".to_string()),
    }
}

fn docker(root: &Path, plan: &ImagePlan) -> Result<()> {
    run_command(
        root,
        &CommandSpec::new("docker").args([
            "build",
            "--tag",
            plan.tag.as_str(),
            "--file",
            plan.file,
            plan.context,
        ]),
    )?;
    docker_smoke(root, &plan.tag, "trunk", &["--version"])?;
    docker_smoke(root, &plan.tag, "test", &["-f", policy::IMAGE_DIST_PATH])
}

fn docker_smoke(root: &Path, tag: &str, program: &str, args: &[&str]) -> Result<()> {
    let name = temporary_name();
    let mut smoke = CommandSpec::new("docker").args([
        "run",
        "--name",
        name.as_str(),
        "--rm",
        "--network",
        "none",
        "--entrypoint",
        program,
        tag,
    ]);
    smoke.args.extend(args.iter().map(Into::into));
    let result = run_command(root, &smoke);
    best_effort(
        root,
        &CommandSpec::new("docker").args(["rm", "--force", name.as_str()]),
    );
    result
}

fn buildah(root: &Path, plan: &ImagePlan) -> Result<()> {
    run_command(
        root,
        &CommandSpec::new("buildah").args([
            "bud",
            "--tag",
            plan.tag.as_str(),
            "--file",
            plan.file,
            plan.context,
        ]),
    )?;

    let name = temporary_name();
    let result = (|| {
        run_command(
            root,
            &CommandSpec::new("buildah").args(["from", "--name", name.as_str(), plan.tag.as_str()]),
        )?;
        run_command(
            root,
            &CommandSpec::new("buildah").args([
                "run",
                "--network",
                "none",
                name.as_str(),
                "--",
                "trunk",
                "--version",
            ]),
        )?;
        run_command(
            root,
            &CommandSpec::new("buildah").args([
                "run",
                "--network",
                "none",
                name.as_str(),
                "--",
                "test",
                "-f",
                policy::IMAGE_DIST_PATH,
            ]),
        )
    })();
    best_effort(
        root,
        &CommandSpec::new("buildah").args(["rm", name.as_str()]),
    );
    result
}

fn temporary_name() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    format!("xtask-smoke-{}-{millis}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_order_has_both_concrete_engines_once() {
        assert_eq!(policy::IMAGE_AUTO_ORDER.len(), 2);
        assert!(policy::IMAGE_AUTO_ORDER.contains(&ImageEngine::Docker));
        assert!(policy::IMAGE_AUTO_ORDER.contains(&ImageEngine::Buildah));
        assert_ne!(policy::IMAGE_AUTO_ORDER[0], policy::IMAGE_AUTO_ORDER[1]);
    }

    #[test]
    fn single_image_uses_the_repository_dockerfile() {
        let planned = plan("yaml-sigil-web:test".to_string());
        assert_eq!(planned.file, "Dockerfile");
        assert_eq!(planned.context, ".");
        assert_eq!(planned.tag, "yaml-sigil-web:test");
    }

    #[test]
    fn temporary_container_name_is_scoped() {
        assert!(temporary_name().starts_with("xtask-smoke-"));
    }
}
