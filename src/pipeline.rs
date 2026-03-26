use std::collections::HashMap;

use crate::bir::instruction::{BirModule, BirType};
use crate::bir::mono::MonoCollectResult;
use crate::package::{ModuleGraph, ModulePath};
use crate::parser::ast::{NodeId, TypeAnnotation};
use crate::semantic::PackageSemanticInfo;

/// Output of the `parse` stage.
pub struct ParsedPackage {
    pub package_name: String,
    pub graph: ModuleGraph,
}

/// Output of the `analyze` stage.
pub struct AnalyzedPackage {
    pub package_name: String,
    pub graph: ModuleGraph,
    pub inferred_maps: HashMap<ModulePath, HashMap<NodeId, Vec<TypeAnnotation>>>,
    pub pkg_sem_info: PackageSemanticInfo,
}

/// Output of the `lower` stage.
pub struct LoweredPackage {
    pub package_name: String,
    pub modules: HashMap<ModulePath, LoweredModule>,
    pub sources: HashMap<ModulePath, String>,
}

pub struct LoweredModule {
    pub bir: BirModule,
    pub is_entry: bool,
}

/// Output of the `monomorphize` stage.
pub struct MonomorphizedPackage {
    pub package_name: String,
    pub modules: HashMap<ModulePath, MonomorphizedModule>,
    pub sources: HashMap<ModulePath, String>,
}

pub struct MonomorphizedModule {
    pub bir: BirModule,
    pub mono_result: MonoCollectResult,
    pub external_functions: Vec<(String, Vec<BirType>, BirType)>,
    pub is_entry: bool,
}

/// Output of the `codegen` stage.
pub struct CompiledPackage {
    pub object_bytes: HashMap<ModulePath, Vec<u8>>,
}

/// Output of `compile_to_bir` / `compile_source_to_bir`.
pub struct BirOutput {
    pub modules: HashMap<ModulePath, LoweredModule>,
    pub bir_texts: HashMap<ModulePath, String>,
}
