mod assertions;
mod marked_text;

use git2;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

pub use assertions::*;
pub use marked_text::*;

/// A temporary filesystem tree constructed from a JSON object.
pub struct TempTree {
    _temp_dir: TempDir,
    path: PathBuf,
}

impl TempTree {
    /// Creates a temporary tree whose object keys are names and whose string
    /// values are file contents; `null` values create empty directories.
    pub fn new(tree: serde_json::Value) -> Self {
        let dir = TempDir::new().unwrap();
        let path = std::fs::canonicalize(dir.path()).unwrap();
        write_tree(path.as_path(), tree);

        Self {
            _temp_dir: dir,
            path,
        }
    }

    /// Returns the root path of the temporary tree.
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }
}

fn write_tree(path: &Path, tree: serde_json::Value) {
    use serde_json::Value;
    use std::fs;

    if let Value::Object(map) = tree {
        for (name, contents) in map {
            let mut path = PathBuf::from(path);
            path.push(name);
            match contents {
                Value::Object(_) => {
                    fs::create_dir(&path).unwrap();

                    if path.file_name() == Some(OsStr::new(".git")) {
                        git2::Repository::init(path.parent().unwrap()).unwrap();
                    }

                    write_tree(&path, contents);
                }
                Value::Null => {
                    fs::create_dir(&path).unwrap();
                }
                Value::String(contents) => {
                    fs::write(&path, contents).unwrap();
                }
                _ => {
                    panic!("JSON object must contain only objects, strings, or null");
                }
            }
        }
    } else {
        panic!("You must pass a JSON object to this helper")
    }
}

/// Generates rectangular sample text with one repeated character per row.
pub fn sample_text(rows: usize, cols: usize, start_char: char) -> String {
    let mut text = String::new();
    for row in 0..rows {
        let c: char = (start_char as u32 + row as u32) as u8 as char;
        let mut line = c.to_string().repeat(cols);
        if row < rows - 1 {
            line.push('\n');
        }
        text += &line;
    }
    text
}
