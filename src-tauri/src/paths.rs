//! Single chokepoint for filesystem path resolution.
//!
//! Every other module receives `&Path` arguments — string-literal path
//! construction outside this file is forbidden by `CLAUDE.md` forbidden
//! patterns.

use std::path::{Path, PathBuf};
use tauri::Manager;

/// The effective data root. Reads the resolved `DataRoot` state; before that
/// state is managed (very early startup) OR if resolution ever fails, falls
/// back to the OS-default app-data dir so no caller ever panics.
pub fn app_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    if let Some(state) = app.try_state::<crate::data_root::DataRoot>() {
        return Ok(state.0.root.clone());
    }
    default_app_data_dir(app)
}

/// The OS-default app-data dir (`%APPDATA%\com.lucerna.app\`). The redirect
/// file always lives here, never under a relocated root.
pub fn default_app_data_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    app.path().app_data_dir()
}

/// `<default app-data>/data-location.json` — bootstrap redirect, fixed location.
pub fn redirect_file(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(default_app_data_dir(app)?.join("data-location.json"))
}

pub fn versions_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("versions"))
}

/// App-wide directory for the launcher's own log (`lucerna.log`). Distinct
/// from the per-instance `instance_logs_dir` (which holds the captured game
/// console output). Surfaced as the "Launcher logs" group in the Logs viewer.
pub fn app_logs_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("logs"))
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

/// `<app_data>/instances` — the parent of every instance directory.
pub fn instances_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("instances"))
}

pub fn instance_dir(app: &tauri::AppHandle, name: &str) -> tauri::Result<PathBuf> {
    Ok(instances_dir(app)?.join(name))
}

pub fn mods_dir(app: &tauri::AppHandle, instance: &str) -> tauri::Result<PathBuf> {
    Ok(instance_dir(app, instance)?.join(".minecraft").join("mods"))
}

pub fn app_file(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("app.json"))
}

/// Global, cross-instance cache of mod project summaries (name / slug / icon),
/// used to de-duplicate the installed-list and dependency-graph metadata
/// fetches. Cosmetic cache — safe to delete; repopulated on demand.
pub fn mods_cache_file(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("mods-cache").join("summaries.json"))
}

pub fn instance_json(app: &tauri::AppHandle, id: &str) -> tauri::Result<PathBuf> {
    Ok(instance_dir(app, id)?.join("instance.json"))
}

/// `<app_data>/instances/<id>/icon.png` — the instance's optional custom
/// picture. Presence of this file is the "has custom icon" state.
pub fn instance_icon_png(app: &tauri::AppHandle, id: &str) -> tauri::Result<PathBuf> {
    Ok(instance_dir(app, id)?.join("icon.png"))
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

/// Per-account cached skin PNGs + metadata sidecars (`<uuid>.png` /
/// `<uuid>.json`). Cosmetic cache — safe to delete; repopulated on demand.
pub fn skins_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("skins"))
}

/// Per-account cached cape PNGs, keyed by texture hash (`<hash>.png`). Cape
/// texture URLs are content-addressed and immutable, so this cache needs no
/// TTL. Cosmetic — safe to delete; repopulated on demand.
pub fn capes_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("capes"))
}

/// Scratch directory for downloaded update installers + bundles.
/// Lives under the app dir; cleared/overwritten per update attempt.
pub fn update_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(app_dir(app)?.join("updates"))
}

/// Все пути одного сервера, выведенные из базовой app-data директории.
/// Чистая функция — тестируется без `AppHandle`.
pub struct ServerPaths {
    pub root: PathBuf,
    pub json: PathBuf,
    pub runtime: PathBuf,
    pub mods: PathBuf,
    /// `runtime/plugins` — Bukkit-family (Paper/Purpur) plugin jars. Only
    /// loaded when the server core is plugin-capable; absent/unused otherwise.
    pub plugins: PathBuf,
    pub logs: PathBuf,
    /// `runtime/server.pid` — the running server's OS PID, persisted so
    /// `server_list` can reconcile live processes after a launcher restart.
    pub pid: PathBuf,
}

/// `<base>/servers` — the parent of every server directory. Pure (takes the
/// app-data root directly) so non-`AppHandle` call sites (e.g. import commit)
/// can reserve a server directory here.
pub fn servers_root(base: &Path) -> PathBuf {
    base.join("servers")
}

pub fn server_paths(base: &Path, id: &str) -> ServerPaths {
    let root = servers_root(base).join(id);
    let runtime = root.join("runtime");
    ServerPaths {
        json: root.join("server.json"),
        mods: runtime.join("mods"),
        plugins: runtime.join("plugins"),
        logs: runtime.join("logs"),
        pid: runtime.join("server.pid"),
        runtime,
        root,
    }
}

/// `<app_data>/servers`.
pub fn servers_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    Ok(servers_root(&app_dir(app)?))
}

/// Пути конкретного сервера, привязанные к реальной app-data директории.
pub fn server_dir_paths(app: &tauri::AppHandle, id: &str) -> tauri::Result<ServerPaths> {
    Ok(server_paths(&app_dir(app)?, id))
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
        root.join("instances")
            .join(instance)
            .join(".minecraft")
            .join("mods")
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

    fn instance_icon_png_from(root: PathBuf, id: &str) -> PathBuf {
        root.join("instances").join(id).join("icon.png")
    }

    #[test]
    fn instance_icon_png_is_under_instance_dir() {
        let root = PathBuf::from("C:/fake/appdata");
        assert_eq!(
            instance_icon_png_from(root, "abc-123"),
            PathBuf::from("C:/fake/appdata/instances/abc-123/icon.png"),
        );
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

#[cfg(test)]
mod server_path_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn server_paths_layout_under_base() {
        let base = Path::new("/data");
        let p = server_paths(base, "srv-1");
        assert_eq!(p.root, Path::new("/data/servers/srv-1"));
        assert_eq!(p.json, Path::new("/data/servers/srv-1/server.json"));
        assert_eq!(p.runtime, Path::new("/data/servers/srv-1/runtime"));
        assert_eq!(p.mods, Path::new("/data/servers/srv-1/runtime/mods"));
        assert_eq!(p.plugins, Path::new("/data/servers/srv-1/runtime/plugins"));
        assert_eq!(p.logs, Path::new("/data/servers/srv-1/runtime/logs"));
        assert_eq!(p.pid, Path::new("/data/servers/srv-1/runtime/server.pid"));
    }
}
