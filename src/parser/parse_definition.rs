use crate::error::{BengalError, Result};
use crate::lexer::token::Token;
use crate::parser::ast::*;

impl super::Parser {
    // --- Visibility helpers ---

    pub(super) fn is_visibility_token(token: &Token) -> bool {
        matches!(
            token,
            Token::Public | Token::Package | Token::Internal | Token::Fileprivate | Token::Private
        )
    }

    pub(super) fn try_parse_visibility(&mut self) -> Visibility {
        match &self.peek().node {
            Token::Public => {
                self.advance();
                Visibility::Public
            }
            Token::Package => {
                self.advance();
                Visibility::Package
            }
            Token::Internal => {
                self.advance();
                Visibility::Internal
            }
            Token::Fileprivate => {
                self.advance();
                Visibility::Fileprivate
            }
            Token::Private => {
                self.advance();
                Visibility::Private
            }
            _ => Visibility::Internal,
        }
    }

    // --- Program / Function ---

    pub(super) fn parse_program(&mut self) -> Result<Program> {
        let mut module_decls = Vec::new();
        let mut import_decls = Vec::new();
        let mut structs = Vec::new();
        let mut protocols = Vec::new();
        let mut functions = Vec::new();

        // Phase 1: Module declarations
        while self.peek().node != Token::Eof {
            let next = if Self::is_visibility_token(&self.peek().node) {
                self.tokens.get(self.pos + 1).map(|t| &t.node)
            } else {
                Some(&self.peek().node)
            };
            if next == Some(&Token::Module) {
                let visibility = self.try_parse_visibility();
                self.expect(Token::Module)?;
                let name = self.expect_ident()?;
                self.expect(Token::Semicolon)?;
                module_decls.push(ModuleDecl { visibility, name });
            } else {
                break;
            }
        }

        // Phase 2: Import declarations
        while self.peek().node != Token::Eof {
            let next = if Self::is_visibility_token(&self.peek().node) {
                self.tokens.get(self.pos + 1).map(|t| &t.node)
            } else {
                Some(&self.peek().node)
            };
            if next == Some(&Token::Import) {
                let visibility = self.try_parse_visibility();
                self.expect(Token::Import)?;
                let mut import_decl = self.parse_import_path()?;
                import_decl.visibility = visibility;
                self.expect(Token::Semicolon)?;
                import_decls.push(import_decl);
            } else {
                break;
            }
        }

        // Phase 3: Top-level declarations with visibility
        while self.peek().node != Token::Eof {
            let visibility = self.try_parse_visibility();
            match self.peek().node {
                Token::Struct => {
                    let mut s = self.parse_struct_def()?;
                    s.visibility = visibility;
                    structs.push(s);
                }
                Token::Protocol => {
                    let mut p = self.parse_protocol_def()?;
                    p.visibility = visibility;
                    protocols.push(p);
                }
                _ => {
                    let mut f = self.parse_function()?;
                    f.visibility = visibility;
                    functions.push(f);
                }
            }
        }

        Ok(Program {
            module_decls,
            import_decls,
            structs,
            protocols,
            functions,
        })
    }

    fn parse_import_path(&mut self) -> Result<ImportDecl> {
        // Parse prefix: self::, super::, or named::
        let prefix = match &self.peek().node {
            Token::SelfKw => {
                self.advance();
                self.expect(Token::ColonColon)?;
                PathPrefix::SelfKw
            }
            Token::Super => {
                self.advance();
                self.expect(Token::ColonColon)?;
                PathPrefix::Super
            }
            _ => {
                let name = self.expect_ident()?;
                self.expect(Token::ColonColon)?;
                PathPrefix::Named(name)
            }
        };

        let mut path = Vec::new();
        let tail = self.parse_import_tail(&mut path)?;

        Ok(ImportDecl {
            visibility: Visibility::Internal,
            prefix,
            path,
            tail,
        })
    }

    /// Expects an identifier or a keyword that can appear as a path segment in imports.
    fn expect_path_ident(&mut self) -> Result<String> {
        let tok = &self.tokens[self.pos];
        let name = match &tok.node {
            Token::Ident(s) => s.clone(),
            // Keywords that may appear as path segments
            Token::Public => "public".to_string(),
            Token::Package => "package".to_string(),
            Token::Internal => "internal".to_string(),
            Token::Fileprivate => "fileprivate".to_string(),
            Token::Private => "private".to_string(),
            Token::Module => "module".to_string(),
            Token::Import => "import".to_string(),
            _ => {
                return Err(BengalError::ParseError {
                    message: format!("expected identifier, found `{}`", tok.node),
                    span: tok.span,
                });
            }
        };
        self.pos += 1;
        Ok(name)
    }

    fn parse_import_tail(&mut self, path: &mut Vec<String>) -> Result<ImportTail> {
        match &self.peek().node {
            Token::Star => {
                self.advance();
                Ok(ImportTail::Glob)
            }
            Token::LBrace => {
                self.advance();
                let mut names = Vec::new();
                names.push(self.expect_path_ident()?);
                while self.peek().node == Token::Comma {
                    self.advance();
                    names.push(self.expect_path_ident()?);
                }
                self.expect(Token::RBrace)?;
                Ok(ImportTail::Group(names))
            }
            _ => {
                let name = self.expect_path_ident()?;
                if self.peek().node == Token::ColonColon {
                    self.advance();
                    path.push(name);
                    self.parse_import_tail(path)
                } else {
                    Ok(ImportTail::Single(name))
                }
            }
        }
    }

    pub(super) fn parse_type_params(&mut self) -> Result<Vec<TypeParam>> {
        let mut params = Vec::new();
        self.expect(Token::Lt)?;
        loop {
            let name = self.expect_ident()?;
            let bound = if self.peek().node == Token::Colon {
                self.advance();
                Some(self.expect_ident()?)
            } else {
                None
            };
            params.push(TypeParam { name, bound });
            if self.peek().node == Token::Comma {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(Token::Gt)?;
        Ok(params)
    }

    pub(super) fn parse_function(&mut self) -> Result<Function> {
        let start = self.current_span_start();
        self.expect(Token::Func)?;
        let name_tok = self.expect(Token::Ident(String::new()))?;
        let name = match &name_tok.node {
            Token::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        let type_params = if self.peek().node == Token::Lt {
            self.parse_type_params()?
        } else {
            vec![]
        };
        let params = self.parse_param_list()?;
        let return_type = if self.peek().node == Token::Arrow {
            self.advance();
            self.parse_type()?
        } else {
            TypeAnnotation::Unit
        };
        let body = self.parse_block()?;
        let span = self.span_from(start);
        Ok(Function {
            visibility: Visibility::Internal,
            name,
            type_params,
            params,
            return_type,
            body,
            span,
        })
    }

    fn parse_struct_def(&mut self) -> Result<StructDef> {
        let start = self.current_span_start();
        self.expect(Token::Struct)?;
        let name = self.expect_ident()?;
        let type_params = if self.peek().node == Token::Lt {
            self.parse_type_params()?
        } else {
            vec![]
        };
        let conformances = if self.peek().node == Token::Colon {
            self.advance(); // consume `:`
            let mut list = vec![self.expect_ident()?];
            while self.peek().node == Token::Comma {
                self.advance();
                list.push(self.expect_ident()?);
            }
            list
        } else {
            vec![]
        };
        self.expect(Token::LBrace)?;
        let mut members = Vec::new();
        while self.peek().node != Token::RBrace {
            members.push(self.parse_struct_member()?);
        }
        self.expect(Token::RBrace)?;
        let span = self.span_from(start);
        Ok(StructDef {
            visibility: Visibility::Internal,
            name,
            type_params,
            conformances,
            members,
            span,
        })
    }

    fn parse_struct_member(&mut self) -> Result<StructMember> {
        let visibility = self.try_parse_visibility();
        match &self.peek().node {
            Token::Var => {
                self.advance();
                let name = self.expect_ident()?;
                self.expect(Token::Colon)?;
                let ty = self.parse_type()?;
                if self.peek().node == Token::LBrace {
                    // Computed property: var name: Type { get { ... } set { ... } };
                    self.advance(); // consume `{`
                    let getter = self.parse_getter()?;
                    let setter = if self.peek().node == Token::RBrace {
                        None
                    } else {
                        Some(self.parse_setter()?)
                    };
                    self.expect(Token::RBrace)?;
                    self.expect(Token::Semicolon)?;
                    Ok(StructMember::ComputedProperty {
                        visibility,
                        name,
                        ty,
                        getter,
                        setter,
                    })
                } else {
                    // Stored property: var name: Type;
                    self.expect(Token::Semicolon)?;
                    Ok(StructMember::StoredProperty {
                        visibility,
                        name,
                        ty,
                    })
                }
            }
            Token::Init => {
                self.advance();
                let params = self.parse_param_list()?;
                let body = self.parse_block()?;
                Ok(StructMember::Initializer {
                    visibility,
                    params,
                    body,
                })
            }
            Token::Func => {
                self.advance(); // consume `func`
                let name = self.expect_ident()?;
                let params = self.parse_param_list()?;
                let return_type = if self.peek().node == Token::Arrow {
                    self.advance();
                    self.parse_type()?
                } else {
                    TypeAnnotation::Unit
                };
                let body = self.parse_block()?;
                Ok(StructMember::Method {
                    visibility,
                    name,
                    params,
                    return_type,
                    body,
                })
            }
            _ => {
                let tok = self.peek();
                Err(BengalError::ParseError {
                    message: format!("expected struct member, found `{}`", tok.node),
                    span: tok.span,
                })
            }
        }
    }

    fn parse_protocol_def(&mut self) -> Result<ProtocolDef> {
        self.expect(Token::Protocol)?;
        let name = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let mut members = Vec::new();
        while self.peek().node != Token::RBrace {
            members.push(self.parse_protocol_member()?);
        }
        self.expect(Token::RBrace)?;
        Ok(ProtocolDef {
            visibility: Visibility::Internal,
            name,
            members,
        })
    }

    fn parse_protocol_member(&mut self) -> Result<ProtocolMember> {
        match &self.peek().node {
            Token::Func => {
                self.advance();
                let name = self.expect_ident()?;
                let params = self.parse_param_list()?;
                let return_type = if self.peek().node == Token::Arrow {
                    self.advance();
                    self.parse_type()?
                } else {
                    TypeAnnotation::Unit
                };
                self.expect(Token::Semicolon)?;
                Ok(ProtocolMember::MethodSig {
                    name,
                    params,
                    return_type,
                })
            }
            Token::Var => {
                self.advance();
                let name = self.expect_ident()?;
                self.expect(Token::Colon)?;
                let ty = self.parse_type()?;
                self.expect(Token::LBrace)?;
                // expect `get` identifier
                let tok = self.expect(Token::Ident(String::new()))?;
                match &tok.node {
                    Token::Ident(s) if s == "get" => {}
                    _ => {
                        return Err(BengalError::ParseError {
                            message: format!("expected `get`, found `{}`", tok.node),
                            span: tok.span,
                        });
                    }
                }
                let has_setter = matches!(&self.peek().node, Token::Ident(s) if s == "set");
                if has_setter {
                    self.advance(); // consume `set`
                }
                self.expect(Token::RBrace)?;
                self.expect(Token::Semicolon)?;
                Ok(ProtocolMember::PropertyReq {
                    name,
                    ty,
                    has_setter,
                })
            }
            _ => {
                let tok = self.peek();
                Err(BengalError::ParseError {
                    message: format!("expected protocol member, found `{}`", tok.node),
                    span: tok.span,
                })
            }
        }
    }

    fn parse_getter(&mut self) -> Result<Block> {
        let tok = self.expect(Token::Ident(String::new()))?;
        match &tok.node {
            Token::Ident(s) if s == "get" => {}
            _ => {
                return Err(BengalError::ParseError {
                    message: format!("expected `get`, found `{}`", tok.node),
                    span: tok.span,
                });
            }
        }
        self.parse_block()
    }

    fn parse_setter(&mut self) -> Result<Block> {
        let tok = self.expect(Token::Ident(String::new()))?;
        match &tok.node {
            Token::Ident(s) if s == "set" => {}
            _ => {
                return Err(BengalError::ParseError {
                    message: format!("expected `set`, found `{}`", tok.node),
                    span: tok.span,
                });
            }
        }
        self.parse_block()
    }
}
