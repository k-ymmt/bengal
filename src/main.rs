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
    /// Compile a .bengal file to .wasm
    Compile {
        /// Source file path
        file: PathBuf,
        /// Print BIR text representation
        #[arg(long)]
        emit_bir: bool,
    },
    /// Evaluate an expression and print the result
    Eval {
        /// Expression to evaluate
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
            let source = std::fs::read_to_string(&file).map_err(|e| miette::miette!("{e}"))?;
            let filename = file.display().to_string();

            if emit_bir {
                let (_module, bir_text) = bengal::compile_to_bir(&source).map_err(|e| {
                    Report::new(e.into_diagnostic(&filename, &source))
                })?;
                println!("{bir_text}");
            }

            let wasm = bengal::compile_source(&source).map_err(|e| {
                Report::new(e.into_diagnostic(&filename, &source))
            })?;

            let out_path = file.with_extension("wasm");
            std::fs::write(&out_path, &wasm).map_err(|e| miette::miette!("{e}"))?;
            eprintln!("Wrote {}", out_path.display());
        }
        Command::Eval { expr, emit_bir } => {
            let source = &expr;
            let filename = "<eval>";

            if emit_bir {
                let (_module, bir_text) = bengal::compile_to_bir(source).map_err(|e| {
                    Report::new(e.into_diagnostic(filename, source))
                })?;
                println!("{bir_text}");
            }

            let wasm = bengal::compile_source(source).map_err(|e| {
                Report::new(e.into_diagnostic(filename, source))
            })?;

            let engine = wasmtime::Engine::default();
            let module = wasmtime::Module::new(&engine, &wasm)
                .map_err(|e| miette::miette!("WASM instantiation error: {e}"))?;
            let mut store = wasmtime::Store::new(&engine, ());
            let instance = wasmtime::Instance::new(&mut store, &module, &[])
                .map_err(|e| miette::miette!("WASM instantiation error: {e}"))?;
            let main_fn = instance
                .get_typed_func::<(), i32>(&mut store, "main")
                .map_err(|e| miette::miette!("failed to find main: {e}"))?;
            let result = main_fn
                .call(&mut store, ())
                .map_err(|e| miette::miette!("WASM execution error: {e}"))?;
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
