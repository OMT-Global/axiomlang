//! Project path resolution helpers shared by the package graph and CLI entrypoints.

use super::canonicalize_existing_path;
use crate::project::Diagnostic;
use std::path::{Component, Path, PathBuf};

/// Resolves a caller-supplied project path with operating-system semantics.
///
/// The path is canonicalized as given, so `.` and a leading `..` are resolved
/// against the current directory and `..` after a symlink follows the link's
/// real parent. Lexically collapsing the path first would silently select a
/// different directory (issue #1779).
pub(super) fn resolve_project_root(project_root: &Path) -> Result<PathBuf, Diagnostic> {
    let requested = if project_root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        project_root
    };
    canonicalize_existing_path(requested, "project root")
}

/// Lexically removes `.` and resolvable `..` components.
///
/// A `..` that has no preceding normal component to cancel is kept on a
/// relative path and ignored at a filesystem root; it is never discarded,
/// because that would retarget the path at a different directory.
pub(super) fn normalize_path(path: impl AsRef<Path>) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.as_ref().components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match normalized.components().next_back() {
                Some(Component::Normal(_)) => {
                    normalized.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                Some(Component::ParentDir | Component::CurDir) | None => {
                    normalized.push(component.as_os_str());
                }
            },
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_path_never_discards_unresolvable_parent_components() {
        assert_eq!(normalize_path("../app"), PathBuf::from("../app"));
        assert_eq!(normalize_path("../../app/./src"), PathBuf::from("../../app/src"));
        assert_eq!(normalize_path("a/../../b"), PathBuf::from("../b"));
        assert_eq!(normalize_path("a/b/../c"), PathBuf::from("a/c"));
        assert_eq!(normalize_path("./a/./b"), PathBuf::from("a/b"));
        #[cfg(unix)]
        {
            assert_eq!(normalize_path("/a/../../b"), PathBuf::from("/b"));
            assert_eq!(normalize_path("/a/b/../c"), PathBuf::from("/a/c"));
        }
    }
}
