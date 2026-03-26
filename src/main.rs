use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use miette::Report;

#[derive(Parser)]
#[command(name = "bengal", about = "The Bengal compiler")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compile a .bengal file to a native executable
    Compile {
        /// Source file path
        file: PathBuf,
        /// Print BIR text representation
        #[arg(long)]
        emit_bir: bool,
    },
    /// Evaluate a Bengal program and print the result
    Eval {
        /// Program or expression to evaluate
        expr: String,
        /// Print BIR text representation
        #[arg(long)]
        emit_bir: bool,
    },
}

fn run() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Compile { file, emit_bir } => {
            let exe_path = file.with_extension("");
            if exe_path == file {
                return Err(miette::miette!(
                    "input file '{}' has no extension; cannot determine output path",
                    file.display()
                ));
            }

            // Run pipeline once, optionally printing BIR
            let parsed =
                bengal::pipeline::parse(&file).map_err(|e| Report::new(e.into_diagnostic()))?;
            let analyzed =
                bengal::pipeline::analyze(parsed).map_err(|e| Report::new(e.into_diagnostic()))?;
            let lowered =
                bengal::pipeline::lower(analyzed).map_err(|e| Report::new(e.into_diagnostic()))?;
            let optimized = bengal::pipeline::optimize(lowered);

            if emit_bir {
                for (mod_path, module) in &optimized.modules {
                    if optimized.modules.len() > 1 {
                        println!("=== module: {} ===", mod_path);
                    }
                    println!("{}", bengal::bir::print_module(&module.bir));
                }
            }

            let mono = bengal::pipeline::monomorphize(optimized)
                .map_err(|e| Report::new(e.into_diagnostic()))?;
            let compiled =
                bengal::pipeline::codegen(mono).map_err(|e| Report::new(e.into_diagnostic()))?;
            bengal::pipeline::link(compiled, &exe_path)
                .map_err(|e| Report::new(e.into_diagnostic()))?;
            eprintln!("Wrote {}", exe_path.display());
        }
        Command::Eval { expr, emit_bir } => {
            let source = &expr;
            let filename = "<eval>";

            let bir_output = bengal::compile_source_to_bir(source)
                .map_err(|e| Report::new(e.into_diagnostic()))?;

            if emit_bir {
                for text in bir_output.bir_texts.values() {
                    println!("{text}");
                }
            }

            // Extract the single module's BIR for JIT execution
            let root_path = bengal::package::ModulePath::root();
            let root_module = bir_output
                .modules
                .get(&root_path)
                .ok_or_else(|| miette::miette!("no root module found"))?;

            let context = inkwell::context::Context::create();
            let module = bengal::codegen::compile_to_module(&context, &root_module.bir)
                .map_err(|e| Report::new(e.into_diagnostic(filename, source)))?;

            let ee = module
                .create_jit_execution_engine(inkwell::OptimizationLevel::None)
                .map_err(|e| miette::miette!("JIT error: {e}"))?;
            let main_fn = unsafe {
                ee.get_function::<unsafe extern "C" fn() -> i32>("main")
                    .map_err(|e| miette::miette!("failed to find main: {e}"))?
            };
            let result = unsafe { main_fn.call() };
            println!("{result}");
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e:?}");
        process::exit(1);
    }
}
