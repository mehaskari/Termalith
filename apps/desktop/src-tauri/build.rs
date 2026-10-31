fn main() {
    // Generate `allow-<command>` permissions for app commands so the
    // capability file can grant them individually (least privilege).
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "terminal_open",
            "terminal_write",
            "terminal_resize",
            "terminal_ack",
            "terminal_close",
        ]),
    ))
    .expect("failed to run tauri-build");
}
