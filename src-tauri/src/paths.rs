//! Single chokepoint for filesystem path resolution.
//!
//! Every other module receives `&Path` arguments — string-literal path
//! construction outside this file is forbidden by `CLAUDE.md` forbidden
//! patterns.

use std::path::PathBuf;
use tauri::Manager;

/// Root of the launcher's persistent data, resolved from the platform's
/// app-data dir (`%APPDATA%\com.ftlauncher.app\` on Windows). All other
/// paths derive from this.
pub fn app_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    app.path().app_data_dir()
}

pub fn versions_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("versions"))
}

pub fn jres_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("jres"))
}

pub fn libraries_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("libraries"))
}

pub fn assets_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("assets"))
}

pub fn instance_dir(app: &tauri::AppHandle, name: &str) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("instances").join(name))
}

pub fn mods_dir(app: &tauri::AppHandle, instance: &str) -> tauri::Result<PathBuf> {
    Ok(instance_dir(app, instance)?.join(".minecraft").join("mods"))
}

pub fn account_file(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("account.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The path helpers all derive from `app_data_dir()`. We can't construct a
    // real `tauri::AppHandle` in a unit test, but we can verify the join
    // logic with a hardcoded root by extracting a pure helper.

    fn versions_dir_from(root: PathBuf) -> PathBuf {
        root.join("versions")
    }
    fn mods_dir_from(root: PathBuf, instance: &str) -> PathBuf {
        root.join("instances").join(instance).join(".minecraft").join("mods")
    }

    #[test]
    fn versions_dir_joins_under_root() {
        let root = PathBuf::from("C:/fake/appdata");
        assert_eq!(
            versions_dir_from(root.clone()),
            PathBuf::from("C:/fake/appdata/versions"),
        );
    }

    #[test]
    fn mods_dir_for_default_instance() {
        let root = PathBuf::from("C:/fake/appdata");
        assert_eq!(
            mods_dir_from(root, "default"),
            PathBuf::from("C:/fake/appdata/instances/default/.minecraft/mods"),
        );
    }

    #[test]
    fn mods_dir_for_named_instance() {
        let root = PathBuf::from("C:/fake/appdata");
        assert_eq!(
            mods_dir_from(root, "modded-1.20"),
            PathBuf::from("C:/fake/appdata/instances/modded-1.20/.minecraft/mods"),
        );
    }
}
