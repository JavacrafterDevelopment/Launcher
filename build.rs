// Embeds the icon and version info into Launcher.exe so Explorer and the taskbar
// show the rocket. That needs the Windows SDK's resource compiler, which MSVC
// builds (including the GitHub release workflow) have; GNU builds skip it and only
// get the window icon, which is set at runtime.

fn main() {
    println!("cargo:rerun-if-changed=assets/launcher.ico");
    println!("cargo:rerun-if-changed=build.rs");

    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows && msvc {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/launcher.ico")
            .set("ProductName", "Launcher")
            .set("FileDescription", "Launcher")
            .set("CompanyName", "Javacrafter Development");
        res.compile().expect("embed Windows resources");
    }
}
