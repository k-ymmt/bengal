pub mod instruction;
pub mod lowering;
pub mod printer;

pub use lowering::lower;
pub use printer::print_module;
