pub mod instruction;
pub mod lowering;
pub mod optimize;
pub mod printer;

pub use lowering::lower_program;
pub use optimize::optimize_module;
pub use printer::print_module;
