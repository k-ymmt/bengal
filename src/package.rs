use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{BengalError, Result};
use crate::parser::ast::Program;

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModulePath(pub Vec<String>);

impl ModulePath {
    pub fn root() -> Self {
        ModulePath(vec![])
    }

    pub fn child(&self, name: &str) -> Self {
        let mut path = self.0.clone();
        path.push(name.to_string());
        ModulePath(path)
    }
}

#[derive(Debug)]
pub struct ModuleInfo {
    pub path: ModulePath,
    pub file_path: PathBuf,
    pub source: String,
    pub ast: Program,
}

#[derive(Debug)]
pub struct ModuleGraph {
    pub modules: HashMap<ModulePath, ModuleInfo>,
}

fn resolve_module_file(parent_dir: &Path, name: &str) -> Result<PathBuf> {
    let file_path = parent_dir.join(format!("{}.bengal", name));
    if file_path.exists() {
        return Ok(file_path);
    }
    let dir_path = parent_dir.join(name).join("module.bengal");
    if dir_path.exists() {
        return Ok(dir_path);
    }
    Err(BengalError::PackageError {
        message: format!(
            "module '{}' not found: expected '{}.bengal' or '{}/module.bengal'",
            name, name, name
        ),
    })
}

pub fn build_module_graph(entry_path: &Path) -> Result<ModuleGraph> {
    let mut modules = HashMap::new();
    let mut finished_files = HashSet::new();
    let mut visiting_stack = Vec::new();

    build_module_recursive(
        entry_path,
        &ModulePath::root(),
        &mut modules,
        &mut finished_files,
        &mut visiting_stack,
    )?;

    Ok(ModuleGraph { modules })
}

fn build_module_recursive(
    file_path: &Path,
    module_path: &ModulePath,
    modules: &mut HashMap<ModulePath, ModuleInfo>,
    finished_files: &mut HashSet<PathBuf>,
    visiting_stack: &mut Vec<PathBuf>,
) -> Result<()> {
    let canonical = file_path
        .canonicalize()
        .map_err(|e| BengalError::PackageError {
            message: format!("failed to resolve path '{}': {}", file_path.display(), e),
        })?;

    // Cycle detection: file is currently being visited (on the stack)
    if visiting_stack.contains(&canonical) {
        let cycle_display: Vec<String> = visiting_stack
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        return Err(BengalError::PackageError {
            message: format!(
                "circular module dependency detected: {} -> {}",
                cycle_display.join(" -> "),
                canonical.display()
            ),
        });
    }

    // Duplicate file detection: file was already fully processed
    if finished_files.contains(&canonical) {
        return Err(BengalError::PackageError {
            message: format!(
                "file '{}' is claimed by multiple modules",
                file_path.display()
            ),
        });
    }

    visiting_stack.push(canonical.clone());

    // Read, lex, and parse the source file
    let source = std::fs::read_to_string(file_path).map_err(|e| BengalError::PackageError {
        message: format!("failed to read '{}': {}", file_path.display(), e),
    })?;
    let tokens = crate::lexer::tokenize(&source)?;
    let ast = crate::parser::parse(tokens)?;

    // Collect child module declarations before moving ast
    let child_decls: Vec<String> = ast.module_decls.iter().map(|d| d.name.clone()).collect();

    // Determine the parent directory for resolving child modules
    let parent_dir = file_path.parent().unwrap_or_else(|| Path::new("."));

    // Insert this module
    modules.insert(
        module_path.clone(),
        ModuleInfo {
            path: module_path.clone(),
            file_path: file_path.to_path_buf(),
            source,
            ast,
        },
    );

    // Recursively process child modules
    for child_name in &child_decls {
        let child_file = resolve_module_file(parent_dir, child_name)?;
        let child_path = module_path.child(child_name);
        build_module_recursive(
            &child_file,
            &child_path,
            modules,
            finished_files,
            visiting_stack,
        )?;
    }

    visiting_stack.pop();
    finished_files.insert(canonical);
    Ok(())
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

    #[test]
    fn build_module_graph_single_module() {
        let dir = TempDir::new().unwrap();
        let main_path = dir.path().join("main.bengal");
        fs::write(
            &main_path,
            "module math; func main() -> Int32 { return 0; }",
        )
        .unwrap();
        let math_path = dir.path().join("math.bengal");
        fs::write(
            &math_path,
            "func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
        )
        .unwrap();

        let graph = build_module_graph(&main_path).unwrap();
        assert_eq!(graph.modules.len(), 2);
        assert!(graph.modules.contains_key(&ModulePath(vec![])));
        assert!(
            graph
                .modules
                .contains_key(&ModulePath(vec!["math".to_string()]))
        );
    }

    #[test]
    fn module_graph_cycle_detection() {
        let dir = TempDir::new().unwrap();
        let a_path = dir.path().join("a.bengal");
        fs::write(&a_path, "module b;").unwrap();
        let b_path = dir.path().join("b.bengal");
        fs::write(&b_path, "module a;").unwrap();

        let result = build_module_graph(&a_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("circular"));
    }

    #[test]
    fn module_graph_missing_file() {
        let dir = TempDir::new().unwrap();
        let main_path = dir.path().join("main.bengal");
        fs::write(&main_path, "module nonexistent;").unwrap();

        let result = build_module_graph(&main_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[test]
    fn module_graph_directory_module() {
        let dir = TempDir::new().unwrap();
        let main_path = dir.path().join("main.bengal");
        fs::write(&main_path, "module graphics;").unwrap();
        let graphics_dir = dir.path().join("graphics");
        fs::create_dir(&graphics_dir).unwrap();
        fs::write(
            graphics_dir.join("module.bengal"),
            "func draw() -> Int32 { return 1; }",
        )
        .unwrap();

        let graph = build_module_graph(&main_path).unwrap();
        assert!(
            graph
                .modules
                .contains_key(&ModulePath(vec!["graphics".to_string()]))
        );
    }
}
