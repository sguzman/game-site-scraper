use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn collect_html_inputs(
    inputs: &[PathBuf],
    recursive: bool,
    follow_symlinks: bool,
) -> Result<Vec<PathBuf>> {
    let mut out: BTreeSet<PathBuf> = BTreeSet::new();

    for p in inputs {
        if p.is_file() {
            if is_html(p) {
                out.insert(p.clone());
            }
            continue;
        }

        if p.is_dir() {
            if recursive {
                for entry in WalkDir::new(p).follow_links(follow_symlinks) {
                    let entry = entry.context("walkdir entry")?;
                    if entry.file_type().is_file() && is_html(entry.path()) {
                        out.insert(entry.path().to_path_buf());
                    }
                }
            } else {
                for entry in
                    std::fs::read_dir(p).with_context(|| format!("read_dir {}", p.display()))?
                {
                    let entry = entry?;
                    let path = entry.path();
                    if path.is_file() && is_html(&path) {
                        out.insert(path);
                    }
                }
            }
        }
    }

    Ok(out.into_iter().collect())
}

fn is_html(path: &Path) -> bool {
    match path.extension().and_then(|s| s.to_str()) {
        Some(ext) => matches!(ext.to_ascii_lowercase().as_str(), "html" | "htm"),
        None => false,
    }
}
