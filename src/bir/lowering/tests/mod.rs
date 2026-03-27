mod test_control_flow;
mod test_expr;
mod test_program;
mod test_struct;

use crate::bir::lowering::lower_program;
use crate::bir::printer::print_module;
use crate::lexer::tokenize;
use crate::parser::parse;
use crate::semantic;

pub(super) fn lower_str(input: &str) -> String {
    let tokens = tokenize(input).unwrap();
    let program = parse(tokens).unwrap();
    let sem_info = semantic::analyze_post_mono(&program).unwrap();
    let module = lower_program(&program, &sem_info).unwrap();
    print_module(&module)
}
