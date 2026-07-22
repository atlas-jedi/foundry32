fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=assets/hangar.ico");
    // On toolchains without a resource compiler (local GNU dev builds) this warns
    // and skips instead of failing; CI/MSVC embeds icon + manifest for real.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        embed_resource::compile("assets/app.rc", embed_resource::NONE)
    })) {
        Ok(_) => {},
        Err(_) => {
            println!("cargo:warning=resource embedding skipped: windres not found");
        }
    }
}
