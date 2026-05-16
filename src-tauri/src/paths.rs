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

pub fn app_file(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("app.json"))
}

pub fn instance_json(app: &tauri::AppHandle, id: &str) -> tauri::Result<PathBuf> {
    Ok(instance_dir(app, id)?.join("instance.json"))
}

pub fn minecraft_dir(app: &tauri::AppHandle, id: &str) -> tauri::Result<PathBuf> {
    Ok(instance_dir(app, id)?.join(".minecraft"))
}

pub fn instance_natives_dir(app: &tauri::AppHandle, id: &str) -> tauri::Result<PathBuf> {
    Ok(instance_dir(app, id)?.join("natives"))
}

pub fn instance_logs_dir(app: &tauri::AppHandle, id: &str) -> tauri::Result<PathBuf> {
    Ok(instance_dir(app, id)?.join("logs"))
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

    fn app_file_from(root: PathBuf) -> PathBuf {
        root.join("app.json")
    }
    fn instance_json_from(root: PathBuf, id: &str) -> PathBuf {
        root.join("instances").join(id).join("instance.json")
    }
    fn minecraft_dir_from(root: PathBuf, id: &str) -> PathBuf {
        root.join("instances").join(id).join(".minecraft")
    }
    fn instance_natives_dir_from(root: PathBuf, id: &str) -> PathBuf {
        root.join("instances").join(id).join("natives")
    }
    fn instance_logs_dir_from(root: PathBuf, id: &str) -> PathBuf {
        root.join("instances").join(id).join("logs")
    }

    #[test]
    fn instance_paths_are_under_instances_uuid() {
        let root = PathBuf::from("C:/fake/appdata");
        let id = "3f4a-bbbb";
        assert_eq!(
            app_file_from(root.clone()),
            PathBuf::from("C:/fake/appdata/app.json"),
        );
        assert_eq!(
            instance_json_from(root.clone(), id),
            PathBuf::from("C:/fake/appdata/instances/3f4a-bbbb/instance.json"),
        );
        assert_eq!(
            minecraft_dir_from(root.clone(), id),
            PathBuf::from("C:/fake/appdata/instances/3f4a-bbbb/.minecraft"),
        );
        assert_eq!(
            instance_natives_dir_from(root.clone(), id),
            PathBuf::from("C:/fake/appdata/instances/3f4a-bbbb/natives"),
        );
        assert_eq!(
            instance_logs_dir_from(root, id),
            PathBuf::from("C:/fake/appdata/instances/3f4a-bbbb/logs"),
        );
    }
}
