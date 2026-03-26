#![allow(unused_assignments)]

use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Error)]
pub enum BengalError {
    #[error("Lex error: {message}")]
    LexError { message: String, span: Span },

    #[error("Parse error: {message}")]
    ParseError { message: String, span: Span },

    #[error("Semantic error: {message}")]
    SemanticError { message: String, span: Span },

    #[error("Lowering error: {message}")]
    LoweringError { message: String, span: Option<Span> },

    #[error("Codegen error: {message}")]
    CodegenError { message: String },

    #[error("Package error: {message}")]
    PackageError { message: String },

    #[error("Interface error: {message}")]
    InterfaceError { message: String },
}

pub type Result<T> = std::result::Result<T, BengalError>;

#[derive(Debug, Error)]
#[error("{phase} error in {module}: {source_error}")]
pub struct PipelineError {
    pub phase: &'static str,
    pub module: String,
    pub source_code: Option<String>,
    pub source_error: BengalError,
}

impl PipelineError {
    pub fn new(phase: &'static str, module: &str, source: Option<&str>, err: BengalError) -> Self {
        PipelineError {
            phase,
            module: module.to_string(),
            source_code: source.map(|s| s.to_string()),
            source_error: err,
        }
    }

    pub fn package(phase: &'static str, err: BengalError) -> Self {
        Self::new(phase, "<package>", None, err)
    }

    pub fn into_diagnostic(self) -> BengalDiagnostic {
        let filename = self.module.clone();
        let source = self.source_code.unwrap_or_default();
        self.source_error.into_diagnostic(&filename, &source)
    }
}

#[derive(Debug, Diagnostic, Error)]
#[error("{message}")]
pub struct BengalDiagnostic {
    pub message: String,
    #[source_code]
    pub src_code: NamedSource<String>,
    #[label("{label}")]
    pub span: Option<SourceSpan>,
    pub label: String,
}

impl BengalError {
    pub fn into_diagnostic(self, filename: &str, source_code: &str) -> BengalDiagnostic {
        let source = NamedSource::new(filename, source_code.to_string());
        match self {
            BengalError::LexError { message, span } => BengalDiagnostic {
                message,
                src_code: source,
                span: Some(SourceSpan::new(span.start.into(), span.end - span.start)),
                label: "here".to_string(),
            },
            BengalError::ParseError { message, span } => BengalDiagnostic {
                message,
                src_code: source,
                span: Some(SourceSpan::new(span.start.into(), span.end - span.start)),
                label: "here".to_string(),
            },
            BengalError::SemanticError { message, span } => BengalDiagnostic {
                message,
                src_code: source,
                span: Some(SourceSpan::new(span.start.into(), span.end - span.start)),
                label: "here".to_string(),
            },
            BengalError::LoweringError { message, span } => BengalDiagnostic {
                message,
                src_code: source,
                span: span.map(|s| SourceSpan::new(s.start.into(), s.end - s.start)),
                label: if span.is_some() {
                    "here".to_string()
                } else {
                    String::new()
                },
            },
            BengalError::CodegenError { message } => BengalDiagnostic {
                message,
                src_code: source,
                span: None,
                label: String::new(),
            },
            BengalError::PackageError { message } => BengalDiagnostic {
                message,
                src_code: source,
                span: None,
                label: String::new(),
            },
            BengalError::InterfaceError { message } => BengalDiagnostic {
                message,
                src_code: source,
                span: None,
                label: String::new(),
            },
        }
    }
}
