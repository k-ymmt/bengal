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
    LoweringError { message: String },

    #[error("Codegen error: {message}")]
    CodegenError { message: String },
}

pub type Result<T> = std::result::Result<T, BengalError>;

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
            BengalError::LoweringError { message } => BengalDiagnostic {
                message,
                src_code: source,
                span: None,
                label: String::new(),
            },
            BengalError::CodegenError { message } => BengalDiagnostic {
                message,
                src_code: source,
                span: None,
                label: String::new(),
            },
        }
    }
}
