fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=assets/witn.ico");

    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "msvc" {
        // Release builds (CI): fail loudly if the resource script does not
        // compile — the shipped GUI exe must carry the comctl32/DPI manifest.
        embed_resource::compile("assets/app.rc", embed_resource::NONE);
        return;
    }

    // Local GNU dev builds may lack a resource compiler; embed-resource panics
    // then. Warn and skip — the app still runs (without visual styles/manifest).
    let attempt = std::panic::catch_unwind(|| {
        embed_resource::compile("assets/app.rc", embed_resource::NONE);
    });
    if attempt.is_err() {
        println!("cargo:warning=resource embedding skipped (no resource compiler on this toolchain)");
    }
}
