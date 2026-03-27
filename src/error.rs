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
    SemanticError {
        message: String,
        span: Span,
        help: Option<String>,
    },

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

/// Diagnostic context for accumulating multiple compilation errors.
pub struct DiagCtxt {
    errors: Vec<BengalError>,
    limit: usize,
}

impl Default for DiagCtxt {
    fn default() -> Self {
        Self {
            errors: Vec::new(),
            limit: 128,
        }
    }
}

impl DiagCtxt {
    /// Create a new diagnostic context with the default error limit (128).
    pub fn new() -> Self {
        Self::default()
    }

    /// Emit an error. Returns false if the limit has been reached.
    pub fn emit(&mut self, err: BengalError) -> bool {
        if self.errors.len() >= self.limit {
            return false;
        }
        self.errors.push(err);
        true
    }

    /// Number of errors emitted so far.
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }

    /// Whether any errors have been emitted.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Consume the context. Returns `Err` with all errors if any were emitted, `Ok(())` otherwise.
    pub fn finish(self) -> std::result::Result<(), Vec<BengalError>> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors)
        }
    }

    /// Take all collected errors, leaving the context empty.
    pub fn take_errors(&mut self) -> Vec<BengalError> {
        std::mem::take(&mut self.errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_error(msg: &str) -> BengalError {
        BengalError::CodegenError {
            message: msg.to_string(),
        }
    }

    #[test]
    fn diag_ctx_starts_empty() {
        let diag = DiagCtxt::new();
        assert_eq!(diag.error_count(), 0);
        assert!(!diag.has_errors());
    }

    #[test]
    fn diag_ctx_emit_increments_count() {
        let mut diag = DiagCtxt::new();
        diag.emit(make_error("e1"));
        assert_eq!(diag.error_count(), 1);
        assert!(diag.has_errors());
        diag.emit(make_error("e2"));
        assert_eq!(diag.error_count(), 2);
    }

    #[test]
    fn diag_ctx_finish_ok_when_no_errors() {
        let diag = DiagCtxt::new();
        assert!(diag.finish().is_ok());
    }

    #[test]
    fn diag_ctx_finish_err_when_has_errors() {
        let mut diag = DiagCtxt::new();
        diag.emit(make_error("boom"));
        let result = diag.finish();
        assert!(result.is_err());
        let errs = result.unwrap_err();
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn diag_ctx_take_errors_leaves_empty() {
        let mut diag = DiagCtxt::new();
        diag.emit(make_error("a"));
        diag.emit(make_error("b"));
        let taken = diag.take_errors();
        assert_eq!(taken.len(), 2);
        assert_eq!(diag.error_count(), 0);
        assert!(!diag.has_errors());
    }

    #[test]
    fn diag_ctx_emit_respects_limit() {
        let mut diag = DiagCtxt::new();
        // Fill to the limit
        for i in 0..128 {
            assert!(diag.emit(make_error(&format!("e{}", i))));
        }
        assert_eq!(diag.error_count(), 128);
        // Next emit should return false and not store
        assert!(!diag.emit(make_error("overflow")));
        assert_eq!(diag.error_count(), 128);
    }

    #[test]
    fn diag_ctx_default_has_limit_128() {
        let mut diag = DiagCtxt::default();
        for i in 0..128 {
            assert!(diag.emit(make_error(&format!("e{}", i))));
        }
        assert!(!diag.emit(make_error("overflow")));
    }
}

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
    #[help]
    pub help: Option<String>,
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
                help: None,
            },
            BengalError::ParseError { message, span } => BengalDiagnostic {
                message,
                src_code: source,
                span: Some(SourceSpan::new(span.start.into(), span.end - span.start)),
                label: "here".to_string(),
                help: None,
            },
            BengalError::SemanticError {
                message,
                span,
                help,
            } => BengalDiagnostic {
                message,
                src_code: source,
                span: Some(SourceSpan::new(span.start.into(), span.end - span.start)),
                label: "here".to_string(),
                help,
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
                help: None,
            },
            BengalError::CodegenError { message } => BengalDiagnostic {
                message,
                src_code: source,
                span: None,
                label: String::new(),
                help: None,
            },
            BengalError::PackageError { message } => BengalDiagnostic {
                message,
                src_code: source,
                span: None,
                label: String::new(),
                help: None,
            },
            BengalError::InterfaceError { message } => BengalDiagnostic {
                message,
                src_code: source,
                span: None,
                label: String::new(),
                help: None,
            },
        }
    }
}
