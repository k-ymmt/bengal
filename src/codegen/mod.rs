pub mod llvm;
mod wasm;

pub use llvm::{compile, compile_to_module};
