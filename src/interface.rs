use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::bir::instruction::BirModule;
use crate::error::{BengalError, Result};
use crate::package::ModulePath;
use crate::pipeline::LoweredPackage;

pub const MAGIC: &[u8; 4] = b"BGMD";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct BengalModFile {
    pub package_name: String,
    pub modules: HashMap<ModulePath, BirModule>,
}

/// Write a LoweredPackage to a .bengalmod interface file.
pub fn write_interface(package: &LoweredPackage, path: &Path) -> Result<()> {
    let modules: HashMap<ModulePath, BirModule> = package
        .modules
        .iter()
        .map(|(k, v)| (k.clone(), v.bir.clone()))
        .collect();

    let file = BengalModFile {
        package_name: package.package_name.clone(),
        modules,
    };

    let payload = rmp_serde::to_vec(&file).map_err(|e| BengalError::InterfaceError {
        message: format!("failed to serialize interface: {}", e),
    })?;

    let mut out = std::fs::File::create(path).map_err(|e| BengalError::InterfaceError {
        message: format!("failed to create file '{}': {}", path.display(), e),
    })?;

    out.write_all(MAGIC)
        .and_then(|()| out.write_all(&FORMAT_VERSION.to_le_bytes()))
        .and_then(|()| out.write_all(&payload))
        .map_err(|e| BengalError::InterfaceError {
            message: format!("failed to write interface file: {}", e),
        })?;

    Ok(())
}

/// Read a .bengalmod interface file.
pub fn read_interface(_path: &Path) -> Result<BengalModFile> {
    todo!("read_interface not yet implemented")
}
