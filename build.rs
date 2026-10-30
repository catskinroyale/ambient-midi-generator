//! Embed the git commit in the binary so every build is identifiable
//! ("0.2.0 (cf32c9d)"). Builds outside a git checkout say "unknown".

use std::path::Path;
use std::process::Command;

fn main() {
    let commit = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=MIDIGEN_GIT_COMMIT={commit}");

    // Rebuild when HEAD moves (new commit or checkout).
    let git = Path::new(".git");
    println!("cargo:rerun-if-changed={}", git.join("HEAD").display());
    if let Ok(head) = std::fs::read_to_string(git.join("HEAD"))
        && let Some(reference) = head.strip_prefix("ref: ")
    {
        println!("cargo:rerun-if-changed={}", git.join(reference.trim()).display());
    }
}
