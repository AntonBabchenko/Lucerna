//! Moving a file or folder to the OS recycle bin — the one place the `trash`
//! crate is called (enforced by `tests/structural_no_raw_spawn.rs`).
//!
//! On macOS the crate's default `DeleteMethod::Finder` asks Finder to delete
//! through an `osascript` process: a spawn outside `process::`, absent from
//! PRINCIPLES Appendix A, and one that can raise an Automation permission
//! prompt. `DeleteMethod::NsFileManager` calls `trashItemAtURL` in-process
//! instead. Trade-off: on some macOS versions the Trash offers no "Put Back"
//! for the item; it can still be dragged out.

use std::path::PathBuf;

/// Move `path` to the OS recycle bin. Blocking: call it through [`off_thread`].
pub fn to_recycle_bin(path: PathBuf) -> std::io::Result<()> {
    context()
        .delete(&path)
        .map_err(|e| std::io::Error::other(e.to_string()))
}

fn context() -> trash::TrashContext {
    trash::TrashContext::new()
}

/// Run a recycle-bin move on the blocking pool. A panic in it (the `trash`
/// crate unwraps a COM call on Windows) is an error, never a crashed command.
pub async fn off_thread<F>(path: PathBuf, recycle: F) -> std::io::Result<()>
where
    F: FnOnce(PathBuf) -> std::io::Result<()> + Send + 'static,
{
    recycle(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn the_mac_context_never_asks_finder() {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        assert!(
            matches!(context().delete_method(), DeleteMethod::NsFileManager),
            "Finder deletes through an `osascript` process"
        );
    }

    #[tokio::test]
    async fn off_thread_hands_the_path_to_the_move() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
        let s = seen.clone();
        let r = off_thread(PathBuf::from("shot.png"), move |p| {
            *s.lock().unwrap() = Some(p);
            Ok(())
        })
        .await;
        assert!(r.is_ok());
        assert_eq!(
            seen.lock().unwrap().as_deref(),
            Some(std::path::Path::new("shot.png"))
        );
    }

    #[tokio::test]
    async fn a_failed_move_is_that_error() {
        let r = off_thread(PathBuf::from("shot.png"), |_| {
            Err(std::io::Error::other("no bin here"))
        })
        .await;
        assert_eq!(r.unwrap_err().to_string(), "no bin here");
    }

    #[tokio::test]
    async fn a_panicking_move_is_an_error_not_a_crash() {
        let r = off_thread(PathBuf::from("shot.png"), |_| {
            panic!("CoCreateInstance failed")
        })
        .await;
        assert!(r.is_err());
    }
}
