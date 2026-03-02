fn main() {
    // Use APP_VERSION env var if set (injected by CI from git tag),
    // otherwise fall back to "dev".
    let version = std::env::var("APP_VERSION").unwrap_or_else(|_| "dev".to_string());
    println!("cargo:rustc-env=APP_VERSION={version}");
    // Re-run if the env var changes
    println!("cargo:rerun-if-env-changed=APP_VERSION");
}
