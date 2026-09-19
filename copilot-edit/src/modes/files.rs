// SPDX-License-Identifier: GPL-3.0-only
//! Create new mode scenes without overwriting files or leaving a partial creation behind.

use std::io::Write;
use std::path::PathBuf;

pub(super) fn with_new_scenes(
    writes: &[(PathBuf, String)],
    commit: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let mut created = Vec::new();
    let result = (|| {
        for (path, body) in writes {
            // The earlier existence check is explanatory, not protection against races.
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
            created.push(path);
            file.write_all(body.as_bytes())
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        }
        commit()
    })();
    if let Err(mut error) = result {
        for path in created {
            if let Err(e) = std::fs::remove_file(path) {
                error.push_str(&format!(
                    "\nCould not remove newly created {}: {e}",
                    path.display()
                ));
            }
        }
        Err(error)
    } else {
        Ok(())
    }
}
