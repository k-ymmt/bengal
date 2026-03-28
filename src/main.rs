use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use miette::Report;

fn parse_dep(s: &str) -> std::result::Result<(String, PathBuf), String> {
    let (name, path) = s
        .split_once('=')
        .ok_or_else(|| format!("invalid dep format '{}', expected name=path.bengalmod", s))?;
    if name.is_empty() {
        return Err("dep name cannot be empty".to_string());
    }
    Ok((name.to_string(), PathBuf::from(path)))
}

fn parse_search_path(s: &str) -> std::result::Result<(String, PathBuf), String> {
    let (kind, path) = s.split_once('=').ok_or_else(|| {
        format!(
            "unsupported -L form: expected '-L bengal=<path>' or '-L native=<path>', got '-L {}'",
            s
        )
    })?;
    match kind {
        "bengal" | "native" => Ok((kind.to_string(), PathBuf::from(path))),
        _ => Err(format!(
            "unsupported -L kind '{}': expected 'bengal' or 'native'",
            kind
        )),
    }
}

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
        /// External dependency: --dep name=path.bengalmod
        #[arg(long = "dep", value_parser = parse_dep)]
        deps: Vec<(String, PathBuf)>,
        /// Sysroot path override
        #[arg(long)]
        sysroot: Option<PathBuf>,
        /// Library search path: -L bengal=<path> or -L native=<path>
        #[arg(short = 'L', value_parser = parse_search_path)]
        search_paths: Vec<(String, PathBuf)>,
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

/// Display all errors collected in DiagCtxt using miette formatting.
fn display_diag_errors(
    diag: &mut bengal::error::DiagCtxt,
    default_filename: &str,
    source_map: &std::collections::HashMap<String, String>,
) {
    let default_source = source_map.get("<root>").cloned().unwrap_or_default();
    for err in diag.take_errors() {
        let diagnostic = err.into_diagnostic(default_filename, &default_source);
        eprintln!("{:?}", Report::new(diagnostic));
    }
}

fn run() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Compile {
            file,
            emit_bir,
            deps,
            sysroot,
            search_paths,
        } => {
            let exe_path = file.with_extension("");
            if exe_path == file {
                return Err(miette::miette!(
                    "input file '{}' has no extension; cannot determine output path",
                    file.display()
                ));
            }

            let filename = file.display().to_string();

            // Run pipeline, collecting multiple errors in diag
            let mut diag = bengal::error::DiagCtxt::new();

            // Build library searcher from --sysroot and -L flags
            let lib_search_paths: Vec<bengal::sysroot::SearchPath> = search_paths
                .into_iter()
                .map(|(kind, path)| {
                    let kind = match kind.as_str() {
                        "bengal" => bengal::sysroot::SearchPathKind::Bengal,
                        "native" => bengal::sysroot::SearchPathKind::Native,
                        _ => unreachable!(),
                    };
                    bengal::sysroot::SearchPath { kind, path }
                })
                .collect();
            let library_searcher = bengal::sysroot::LibrarySearcher::new(sysroot, lib_search_paths);

            // Validate -L bengal= paths exist (user-provided only, not sysroot)
            for path in library_searcher.user_bengal_search_paths() {
                if !path.is_dir() {
                    return Err(miette::miette!(
                        "-L bengal= path '{}' does not exist or is not a directory",
                        path.display()
                    ));
                }
            }

            let parsed =
                bengal::pipeline::parse(&file).map_err(|e| Report::new(e.into_diagnostic()))?;

            // Load external dependencies
            let mut external_deps = Vec::new();
            let mut seen_dep_names = std::collections::HashSet::new();
            for (name, path) in &deps {
                if !seen_dep_names.insert(name.clone()) {
                    return Err(miette::miette!("--dep '{}' specified multiple times", name));
                }
                let dep = bengal::pipeline::load_external_dep(name, path)
                    .map_err(|e| Report::new(e.into_diagnostic()))?;
                external_deps.push(dep);
            }

            // Auto-discover dependencies from search paths
            match bengal::pipeline::pre_scan_imports(
                &parsed.graph,
                &seen_dep_names,
                &library_searcher,
            ) {
                Ok(discovered) => external_deps.extend(discovered),
                Err(e) => return Err(Report::new(e.into_diagnostic())),
            }

            // Capture source texts for error display
            let source_map: std::collections::HashMap<String, String> = parsed
                .graph
                .modules
                .iter()
                .map(|(path, info)| (path.to_string(), info.source.clone()))
                .collect();

            let analyzed = bengal::pipeline::analyze_with_deps(parsed, &external_deps, &mut diag);
            if let Err(ref _e) = analyzed {
                if diag.has_errors() {
                    display_diag_errors(&mut diag, &filename, &source_map);
                    process::exit(1);
                }
            }
            let analyzed = analyzed.map_err(|e| Report::new(e.into_diagnostic()))?;

            let lowered = bengal::pipeline::lower(analyzed, &mut diag);
            if let Err(ref _e) = lowered {
                if diag.has_errors() {
                    display_diag_errors(&mut diag, &filename, &source_map);
                    process::exit(1);
                }
            }
            let lowered = lowered.map_err(|e| Report::new(e.into_diagnostic()))?;

            // Emit per-module interfaces (before merging external deps)
            bengal::pipeline::emit_interfaces(&lowered, std::path::Path::new(".build/cache"));

            // Merge external dep BIR into lowered package
            let mut lowered = lowered;
            bengal::pipeline::merge_external_deps(&mut lowered, &external_deps);

            let optimized = bengal::pipeline::optimize(lowered);

            if emit_bir {
                for (mod_path, module) in &optimized.modules {
                    if optimized.modules.len() > 1 {
                        println!("=== module: {} ===", mod_path);
                    }
                    println!("{}", bengal::bir::print_module(&module.bir));
                }
            }

            let emit_data = bengal::pipeline::EmitData::from_lowered(&optimized);
            let mono = bengal::pipeline::monomorphize(optimized, &mut diag)
                .map_err(|e| Report::new(e.into_diagnostic()))?;
            let compiled = bengal::pipeline::codegen(mono, &mut diag);
            if let Err(ref _e) = compiled {
                if diag.has_errors() {
                    display_diag_errors(&mut diag, &filename, &source_map);
                    process::exit(1);
                }
            }
            let compiled = compiled.map_err(|e| Report::new(e.into_diagnostic()))?;

            bengal::pipeline::emit_package_bengalmod(
                &emit_data,
                std::path::Path::new(".build/cache"),
            );

            let ext_objects = bengal::pipeline::collect_external_objects(&external_deps);
            bengal::pipeline::link(
                compiled,
                &ext_objects,
                &exe_path,
                library_searcher.native_search_paths(),
            )
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
