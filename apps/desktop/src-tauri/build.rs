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
            "ssh_open",
            "host_key_respond",
            "app_info",
            "sessions_list",
            "session_create",
            "session_update",
            "session_delete",
            "session_forget_secret",
            "sftp_home",
            "sftp_list",
            "sftp_download",
            "sftp_upload",
            "transfer_cancel",
            "known_host_forget",
            "ssh_config_preview",
            "ssh_config_import",
        ]),
    ))
    .expect("failed to run tauri-build");
}
