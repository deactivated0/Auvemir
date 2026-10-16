fn main() {
    slint_build::compile("../../ui/app.slint").expect("Slint UI must compile");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../packaging/icons/auvemir.ico")
            .set_manifest_file("../../packaging/windows.manifest")
            .set("ProductName", "Auvemir")
            .set("FileDescription", "Auvemir audio player")
            .compile()
            .expect("Windows application resources must compile");
    }
}
