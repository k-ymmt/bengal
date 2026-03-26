pub mod instruction;
pub mod lowering;
pub mod mono;
pub mod optimize;
pub mod printer;

pub use lowering::lower_module;
pub use lowering::lower_program;
pub use lowering::semantic_type_to_bir;
pub use optimize::optimize_module;
pub use printer::print_module;
