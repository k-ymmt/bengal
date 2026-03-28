use std::path::{Path, PathBuf};

pub struct SearchPath {
    pub kind: SearchPathKind,
    pub path: PathBuf,
}

pub enum SearchPathKind {
    Bengal,
    Native,
}

pub struct LibrarySearcher {
    bengal_search_paths: Vec<PathBuf>,
    native_search_paths: Vec<PathBuf>,
    user_bengal_path_count: usize,
}

impl LibrarySearcher {
    pub fn new(sysroot_override: Option<PathBuf>, search_paths: Vec<SearchPath>) -> Self {
        let mut bengal_search_paths = Vec::new();
        let mut native_search_paths = Vec::new();

        for sp in search_paths {
            match sp.kind {
                SearchPathKind::Bengal => bengal_search_paths.push(sp.path),
                SearchPathKind::Native => native_search_paths.push(sp.path),
            }
        }

        let user_bengal_path_count = bengal_search_paths.len();

        // Sysroot appended last (lowest priority)
        if let Some(sysroot) = Self::resolve_sysroot(sysroot_override) {
            bengal_search_paths.push(Self::sysroot_lib_path(&sysroot));
        }

        Self {
            bengal_search_paths,
            native_search_paths,
            user_bengal_path_count,
        }
    }

    pub fn find_bengalmod(&self, name: &str) -> Option<PathBuf> {
        let filename = format!("{}.bengalmod", name);
        for dir in &self.bengal_search_paths {
            let candidate = dir.join(&filename);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    pub fn native_search_paths(&self) -> &[PathBuf] {
        &self.native_search_paths
    }

    pub fn bengal_search_paths(&self) -> &[PathBuf] {
        &self.bengal_search_paths
    }

    /// Returns only user-specified -L bengal= paths (excludes sysroot).
    pub fn user_bengal_search_paths(&self) -> &[PathBuf] {
        &self.bengal_search_paths[..self.user_bengal_path_count]
    }

    fn resolve_sysroot(override_path: Option<PathBuf>) -> Option<PathBuf> {
        let sysroot = match override_path {
            Some(path) => path,
            None => {
                let exe = std::env::current_exe().ok()?;
                exe.parent()?.parent()?.to_path_buf()
            }
        };
        let lib_path = Self::sysroot_lib_path(&sysroot);
        if lib_path.is_dir() {
            Some(sysroot)
        } else {
            None
        }
    }

    fn target_triple() -> String {
        inkwell::targets::TargetMachine::get_default_triple()
            .as_str()
            .to_string_lossy()
            .into_owned()
    }

    fn sysroot_lib_path(sysroot: &Path) -> PathBuf {
        let triple = Self::target_triple();
        sysroot
            .join("lib")
            .join("bengallib")
            .join(triple)
            .join("lib")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_sysroot_with_lib(name: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        let triple = LibrarySearcher::target_triple();
        let lib_dir = dir
            .path()
            .join("lib")
            .join("bengallib")
            .join(&triple)
            .join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();
        std::fs::write(lib_dir.join(format!("{}.bengalmod", name)), b"dummy").unwrap();
        dir
    }

    fn create_search_dir_with_lib(name: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(format!("{}.bengalmod", name)), b"dummy").unwrap();
        dir
    }

    #[test]
    fn sysroot_override_takes_priority() {
        let sysroot = create_sysroot_with_lib("Core");
        let searcher = LibrarySearcher::new(Some(sysroot.path().to_path_buf()), vec![]);
        assert!(searcher.find_bengalmod("Core").is_some());
    }

    #[test]
    fn find_bengalmod_in_bengal_search_path() {
        let search_dir = create_search_dir_with_lib("MyLib");
        let searcher = LibrarySearcher::new(
            None,
            vec![SearchPath {
                kind: SearchPathKind::Bengal,
                path: search_dir.path().to_path_buf(),
            }],
        );
        assert!(searcher.find_bengalmod("MyLib").is_some());
    }

    #[test]
    fn bengal_search_path_before_sysroot() {
        let sysroot = create_sysroot_with_lib("Foo");
        let search_dir = create_search_dir_with_lib("Foo");
        let searcher = LibrarySearcher::new(
            Some(sysroot.path().to_path_buf()),
            vec![SearchPath {
                kind: SearchPathKind::Bengal,
                path: search_dir.path().to_path_buf(),
            }],
        );
        let found = searcher.find_bengalmod("Foo").unwrap();
        assert_eq!(found, search_dir.path().join("Foo.bengalmod"));
    }

    #[test]
    fn returns_none_when_not_found() {
        let searcher = LibrarySearcher::new(None, vec![]);
        assert!(searcher.find_bengalmod("NonExistent").is_none());
    }

    #[test]
    fn native_search_paths_separated() {
        let searcher = LibrarySearcher::new(
            None,
            vec![
                SearchPath {
                    kind: SearchPathKind::Native,
                    path: PathBuf::from("/usr/lib"),
                },
                SearchPath {
                    kind: SearchPathKind::Bengal,
                    path: PathBuf::from("/opt/bengal/lib"),
                },
            ],
        );
        assert_eq!(searcher.native_search_paths(), &[PathBuf::from("/usr/lib")]);
        assert_eq!(
            searcher.bengal_search_paths(),
            &[PathBuf::from("/opt/bengal/lib")]
        );
    }

    #[test]
    fn sysroot_resolution_silent_fallback_on_missing_dir() {
        let dir = TempDir::new().unwrap();
        let searcher = LibrarySearcher::new(Some(dir.path().to_path_buf()), vec![]);
        assert!(searcher.find_bengalmod("Core").is_none());
    }

    #[test]
    fn multiple_bengal_search_paths_first_wins() {
        let dir1 = create_search_dir_with_lib("Dup");
        let dir2 = create_search_dir_with_lib("Dup");
        let searcher = LibrarySearcher::new(
            None,
            vec![
                SearchPath {
                    kind: SearchPathKind::Bengal,
                    path: dir1.path().to_path_buf(),
                },
                SearchPath {
                    kind: SearchPathKind::Bengal,
                    path: dir2.path().to_path_buf(),
                },
            ],
        );
        let found = searcher.find_bengalmod("Dup").unwrap();
        assert_eq!(found, dir1.path().join("Dup.bengalmod"));
    }
}
