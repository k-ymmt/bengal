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
            let source_path = std::fs::canonicalize(&file).map_err(|e| miette::miette!("{e}"))?;
            let start_dir = source_path.parent().unwrap();

            if let Some(package_root) =
                bengal::package::find_package_root(start_dir).map_err(|e| {
                    let diag = e.into_diagnostic("<package>", "");
                    Report::new(diag)
                })?
            {
                // Package mode
                let config = bengal::package::load_package(&package_root).map_err(|e| {
                    let diag = e.into_diagnostic("<package>", "");
                    Report::new(diag)
                })?;
                let entry_path = package_root.join(&config.package.entry);

                if !entry_path.exists() {
                    return Err(miette::miette!(
                        "entry file '{}' not found",
                        config.package.entry
                    ));
                }

                if emit_bir {
                    // For BIR output in package mode, just compile the entry file in single-file mode
                    let source =
                        std::fs::read_to_string(&entry_path).map_err(|e| miette::miette!("{e}"))?;
                    let filename = entry_path.display().to_string();
                    let (_module, bir_text) = bengal::compile_to_bir(&source)
                        .map_err(|e| Report::new(e.into_diagnostic(&filename, &source)))?;
                    println!("{bir_text}");
                }

                let exe_path = entry_path.with_extension("");
                bengal::compile_package_to_executable(&entry_path, &exe_path).map_err(|e| {
                    let diag = e.into_diagnostic("<package>", "");
                    Report::new(diag)
                })?;
                eprintln!("Wrote {}", exe_path.display());
            } else {
                // Single-file mode (existing behavior)
                let source = std::fs::read_to_string(&file).map_err(|e| miette::miette!("{e}"))?;
                let filename = file.display().to_string();

                if emit_bir {
                    let (_module, bir_text) = bengal::compile_to_bir(&source)
                        .map_err(|e| Report::new(e.into_diagnostic(&filename, &source)))?;
                    println!("{bir_text}");
                }

                let obj_bytes = bengal::compile_source(&source)
                    .map_err(|e| Report::new(e.into_diagnostic(&filename, &source)))?;

                let obj_path = file.with_extension("o");
                std::fs::write(&obj_path, &obj_bytes).map_err(|e| miette::miette!("{e}"))?;

                let exe_path = file.with_extension("");
                if exe_path == file {
                    return Err(miette::miette!(
                        "input file '{}' has no extension; cannot determine output path",
                        file.display()
                    ));
                }
                let status = std::process::Command::new("cc")
                    .arg(&obj_path)
                    .arg("-o")
                    .arg(&exe_path)
                    .status()
                    .map_err(|e| miette::miette!("{e}"))?;
                if !status.success() {
                    return Err(miette::miette!("linker failed"));
                }
                eprintln!("Wrote {}", exe_path.display());
            }
        }
        Command::Eval { expr, emit_bir } => {
            let source = &expr;
            let filename = "<eval>";

            let (mut bir, bir_text) = bengal::compile_to_bir(source)
                .map_err(|e| Report::new(e.into_diagnostic(filename, source)))?;

            if emit_bir {
                println!("{bir_text}");
            }

            bengal::bir::optimize_module(&mut bir);

            let context = inkwell::context::Context::create();
            let module = bengal::codegen::compile_to_module(&context, &bir)
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
