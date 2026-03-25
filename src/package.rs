use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{BengalError, Result};

#[derive(Debug, Deserialize)]
pub struct PackageConfig {
    pub package: PackageSection,
}

#[derive(Debug, Deserialize)]
pub struct PackageSection {
    pub name: String,
    pub entry: String,
}

pub fn find_package_root(start: &Path) -> Result<Option<PathBuf>> {
    let mut current = start.to_path_buf();
    loop {
        if current.join("Bengal.toml").exists() {
            return Ok(Some(current));
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

pub fn parse_package_config(content: &str) -> Result<PackageConfig> {
    toml::from_str(content).map_err(|e| BengalError::PackageError {
        message: format!("failed to parse Bengal.toml: {}", e),
    })
}

pub fn load_package(root: &Path) -> Result<PackageConfig> {
    let toml_path = root.join("Bengal.toml");
    let content = std::fs::read_to_string(&toml_path).map_err(|e| BengalError::PackageError {
        message: format!("failed to read {}: {}", toml_path.display(), e),
    })?;
    parse_package_config(&content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn find_bengal_toml_in_same_dir() {
        let dir = TempDir::new().unwrap();
        let toml_path = dir.path().join("Bengal.toml");
        fs::write(
            &toml_path,
            "[package]\nname = \"test\"\nentry = \"main.bengal\"",
        )
        .unwrap();
        let result = find_package_root(dir.path()).unwrap();
        assert_eq!(result, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn find_bengal_toml_in_parent_dir() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("src");
        fs::create_dir(&sub).unwrap();
        let toml_path = dir.path().join("Bengal.toml");
        fs::write(
            &toml_path,
            "[package]\nname = \"test\"\nentry = \"src/main.bengal\"",
        )
        .unwrap();
        let result = find_package_root(&sub).unwrap();
        assert_eq!(result, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn no_bengal_toml_returns_none() {
        let dir = TempDir::new().unwrap();
        let result = find_package_root(dir.path()).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn parse_bengal_toml() {
        let content = "[package]\nname = \"my_app\"\nentry = \"src/main.bengal\"";
        let config = parse_package_config(content).unwrap();
        assert_eq!(config.package.name, "my_app");
        assert_eq!(config.package.entry, "src/main.bengal");
    }
}
