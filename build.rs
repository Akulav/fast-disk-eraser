fn main() {
    // Embed the app icon and version info into the Windows executable (Explorer, taskbar, Task Manager).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Fast Disk Eraser");
        res.set("FileDescription", "Fast Disk Eraser - made by Akulav");
        res.set("CompanyName", "Akulav");
        res.set("LegalCopyright", "© Akulav. All rights reserved.");
        res.set("OriginalFilename", "FastDiskEraser.exe");
        res.set("InternalName", "FastDiskEraser");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed Windows resources: {e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
