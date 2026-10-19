//! On Windows, puts the icon and version details into spotless.exe, for
//! Explorer, the taskbar and Alt+Tab.

fn main() {
    println!("cargo:rerun-if-changed=packaging/icons/spotless.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("packaging/icons/spotless.ico")
        .set("ProductName", "Spotless")
        .set("FileDescription", "Spotless");
    if let Err(error) = resource.compile() {
        panic!("unable to embed the Windows icon: {error}");
    }
}
