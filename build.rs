fn main() {
    // Expose GIT info and build time to the binary
    let git_desc = std::process::Command::new("git")
        .args(["describe", "--always", "--dirty", "--tags"])
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(String::from_utf8_lossy(&o.stdout).trim().to_string()) } else { None });

    if let Some(desc) = git_desc {
        println!("cargo:rustc-env=GIT_DESC={}", desc);
    }

    let git_hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(String::from_utf8_lossy(&o.stdout).trim().to_string()) } else { None });
    if let Some(hash) = git_hash {
        println!("cargo:rustc-env=GIT_HASH={}", hash);
    }

    println!("cargo:rustc-env=BUILD_TIME={}", chrono::Utc::now().to_rfc3339());
    println!("cargo:rerun-if-changed=build.rs");
}


