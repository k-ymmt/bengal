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
            let mut diag = bengal::error::DiagCtxt::new();
            let parsed =
                bengal::pipeline::parse(&file).map_err(|e| Report::new(e.into_diagnostic()))?;
            let analyzed = bengal::pipeline::analyze(parsed, &mut diag)
                .map_err(|e| Report::new(e.into_diagnostic()))?;
            let lowered = bengal::pipeline::lower(analyzed, &mut diag)
                .map_err(|e| Report::new(e.into_diagnostic()))?;
            let optimized = bengal::pipeline::optimize(lowered);

            if emit_bir {
                for (mod_path, module) in &optimized.modules {
                    if optimized.modules.len() > 1 {
                        println!("=== module: {} ===", mod_path);
                    }
                    println!("{}", bengal::bir::print_module(&module.bir));
                }
            }

            let mono = bengal::pipeline::monomorphize(optimized, &mut diag)
                .map_err(|e| Report::new(e.into_diagnostic()))?;
            let compiled = bengal::pipeline::codegen(mono, &mut diag)
                .map_err(|e| Report::new(e.into_diagnostic()))?;
            bengal::pipeline::link(compiled, &exe_path)
                .map_err(|e| Report::new(e.into_diagnostic()))?;
            eprintln!("Wrote {}", exe_path.display());
        }
        Command::Eval { expr, emit_bir } => {
            let source = &expr;

            if emit_bir {
                let bir_output = bengal::compile_source_to_bir(source)
                    .map_err(|e| Report::new(e.into_diagnostic()))?;
                for text in bir_output.bir_texts.values() {
                    println!("{text}");
                }
            }

            let obj_bytes = bengal::compile_source_to_objects(source)
                .map_err(|e| Report::new(e.into_diagnostic("<eval>", source)))?;

            let dir = std::env::temp_dir().join(format!("bengal_eval_{}", std::process::id()));
            std::fs::create_dir_all(&dir)
                .map_err(|e| miette::miette!("failed to create temp dir: {e}"))?;
            let obj_path = dir.join("eval.o");
            let exe_path = dir.join("eval");

            std::fs::write(&obj_path, &obj_bytes)
                .map_err(|e| miette::miette!("failed to write object file: {e}"))?;

            let link = process::Command::new("cc")
                .arg(&obj_path)
                .arg("-o")
                .arg(&exe_path)
                .output()
                .map_err(|e| miette::miette!("cc not found: {e}"))?;
            if !link.status.success() {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(miette::miette!(
                    "link failed: {}",
                    String::from_utf8_lossy(&link.stderr)
                ));
            }

            let run = process::Command::new(&exe_path)
                .output()
                .map_err(|e| miette::miette!("failed to execute binary: {e}"))?;

            let _ = std::fs::remove_dir_all(&dir);

            let code = run
                .status
                .code()
                .ok_or_else(|| miette::miette!("process terminated by signal"))?;
            println!("{code}");
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
