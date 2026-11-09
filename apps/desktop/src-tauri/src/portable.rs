//! Portable mode (ADR-033): a folder named `TermalithData` next to the app
//! holds everything the app stores, so it runs from a USB stick without
//! touching the computer's own settings or credential store.

use std::path::{Path, PathBuf};

pub const DATA_FOLDER: &str = "TermalithData";

/// Where the portable folder would be: next to the program the user sees
/// (the `.exe`, the AppImage file, the `Termalith.app` bundle).
pub fn folder_for(exe: &Path, appimage: Option<&Path>) -> Option<PathBuf> {
    // An AppImage runs from a temporary mount; its own file is in $APPIMAGE.
    let shown = appimage.unwrap_or(exe);
    let mut dir = shown.parent()?;
    // macOS: .../Termalith.app/Contents/MacOS/termalith-desktop
    if dir.file_name().is_some_and(|n| n == "MacOS")
        && let Some(contents) = dir
            .parent()
            .filter(|c| c.file_name().is_some_and(|n| n == "Contents"))
        && let Some(bundle) = contents
            .parent()
            .filter(|b| b.extension().is_some_and(|e| e == "app"))
    {
        dir = bundle.parent()?;
    }
    Some(dir.join(DATA_FOLDER))
}

/// The portable data folder, if the user created one next to the app.
pub fn data_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
    folder_for(&exe, appimage.as_deref()).filter(|d| d.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_folder_next_to_what_the_user_sees() {
        assert_eq!(
            folder_for(Path::new("/usb/Termalith/termalith-desktop.exe"), None),
            Some(PathBuf::from("/usb/Termalith/TermalithData"))
        );
        assert_eq!(
            folder_for(
                Path::new("/usb/Termalith.app/Contents/MacOS/termalith-desktop"),
                None
            ),
            Some(PathBuf::from("/usb/TermalithData"))
        );
        assert_eq!(
            folder_for(
                Path::new("/tmp/.mount_x/usr/bin/termalith-desktop"),
                Some(Path::new("/media/usb/Termalith.AppImage"))
            ),
            Some(PathBuf::from("/media/usb/TermalithData"))
        );
        // A MacOS folder outside an app bundle is not special.
        assert_eq!(
            folder_for(Path::new("/x/MacOS/termalith-desktop"), None),
            Some(PathBuf::from("/x/MacOS/TermalithData"))
        );
    }
}
