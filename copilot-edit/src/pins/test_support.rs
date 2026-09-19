// SPDX-License-Identifier: GPL-3.0-only
//! Isolated project-local rig fixtures, removed when each test finishes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A unique fixture directory whose rig path also accepts ordinary filesystem operations.
pub(crate) struct Fixture {
    path: PathBuf,
}

impl Fixture {
    /// Allocate a per-test directory under the project's build output.
    pub(crate) fn new(tag: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::current_dir().unwrap().join("target");
        std::fs::create_dir_all(&root).unwrap();
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = root.join(format!(
            "gpio-test-{tag}-{}-{time}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        Self {
            path: dir.join("test.rig"),
        }
    }
}

impl std::ops::Deref for Fixture {
    type Target = PathBuf;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl AsRef<Path> for Fixture {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// An app whose test files stay alive until its assertions are finished.
pub(crate) struct TestApp {
    app: crate::App,
    _files: Fixture,
}

impl TestApp {
    /// Open a test rig while retaining ownership of its files.
    pub(crate) fn open(files: Fixture) -> Self {
        let mut app = crate::App::new(None, &[]);
        app.open_rig(files.path.clone());
        assert!(app.rig.is_some(), "{}", app.status);
        Self { app, _files: files }
    }
}

impl std::ops::Deref for TestApp {
    type Target = crate::App;

    fn deref(&self) -> &Self::Target {
        &self.app
    }
}

impl std::ops::DerefMut for TestApp {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.app
    }
}
