//! Watching the `DWIN_SET` folder, so the window reloads it when a build
//! rewrites it.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, SystemTime};

use crate::link::Waker;

/// How often the folder is looked at.
const POLL: Duration = Duration::from_millis(300);

/// Each file's name, length and modification time.
type Fingerprint = Vec<(String, u64, Option<SystemTime>)>;

/// Reports when the folder has changed and has stayed the same since, so a
/// folder being written is not read halfway.
pub(crate) struct Watcher {
    changed: Receiver<()>,
}

impl Watcher {
    pub fn new(folder: PathBuf, wake: Waker) -> std::io::Result<Self> {
        let (sender, changed) = channel();
        std::thread::Builder::new()
            .name("degus sim watch".into())
            .spawn(move || {
                let mut known = fingerprint(&folder);
                let mut seen = known.clone();
                loop {
                    std::thread::sleep(POLL);
                    let now = fingerprint(&folder);
                    // Changed, and the same as one poll ago: the writing is done.
                    if now != known && now == seen {
                        known = now.clone();
                        if sender.send(()).is_err() {
                            return;
                        }
                        wake();
                    }
                    seen = now;
                }
            })?;
        Ok(Self { changed })
    }

    /// Whether the folder changed since the last call.
    pub fn changed(&self) -> bool {
        self.changed.try_iter().count() > 0
    }
}

fn fingerprint(folder: &Path) -> Fingerprint {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Fingerprint = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            Some((name, meta.len(), meta.modified().ok()))
        })
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use super::*;

    #[test]
    fn reports_a_change_once_it_settles() {
        let folder = std::env::temp_dir().join(format!("degus-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("13TouchFile.bin"), [1]).unwrap();
        let watcher = Watcher::new(folder.clone(), Arc::new(|| {})).unwrap();
        std::thread::sleep(POLL * 2);
        assert!(!watcher.changed());

        std::fs::write(folder.join("13TouchFile.bin"), [1, 2]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !watcher.changed() {
            assert!(Instant::now() < deadline, "no change reported");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
