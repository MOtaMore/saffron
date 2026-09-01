//! Embeds the Windows executable icon (`assets/icon.ico`) into the built `.exe`
//! so it shows up in Explorer, the taskbar and shortcuts. No-op on other
//! platforms; the in-game window icon is set at runtime in `main.rs`.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=no se pudo incrustar assets/icon.ico: {e}");
        }
    }
}
