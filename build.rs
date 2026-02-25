use std::process::Command;

fn main() {
    // DEV_HUD_VERSION: GoReleaser can set this env var at build time via:
    //   env: ["DEV_HUD_VERSION={{ .Version }}"]
    // Falls back to CARGO_PKG_VERSION (from Cargo.toml) for local builds.
    let version = std::env::var("DEV_HUD_VERSION")
        .unwrap_or_else(|_| std::env::var("CARGO_PKG_VERSION").unwrap_or_default());
    println!("cargo:rustc-env=DEV_HUD_VERSION={version}");

    // DEV_HUD_COMMIT: GoReleaser can set this env var at build time via:
    //   env: ["DEV_HUD_COMMIT={{ .ShortCommit }}"]
    // Falls back to `git rev-parse --short HEAD` for local builds.
    let commit = std::env::var("DEV_HUD_COMMIT").unwrap_or_else(|_| {
        let output = Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                String::from_utf8_lossy(&o.stdout).trim().to_string()
            }
            _ => "unknown".to_string(),
        }
    });
    println!("cargo:rustc-env=DEV_HUD_COMMIT={commit}");

    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads/");
}
