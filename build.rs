fn main() {
    // Embed the app icon and version details so Windows shows "Remote Access Crew" with its logo.
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/app.rc");
        println!("cargo:rerun-if-changed=assets/remote-access-crew.ico");
        embed_resource::compile("assets/app.rc", embed_resource::NONE).manifest_optional().unwrap();
    }
}
