use crate::token::{Token, TokenKind};
use crate::types::*;
use crate::symtab::*;
use crate::printer;
use crate::ast::AstNode;
use crate::quad::{FunctionQuads, SymClass};

fn classify_sym(scope_idx: usize, sym: &Symbol) -> SymClass {
    if scope_idx == 0 {
        SymClass::Global
    } else if let Some(n) = sym.arg_num {
        SymClass::Param(n)
    } else {
        SymClass::Lvar
    }
}

#[derive(Clone)]
pub struct GlobalDecl {
    pub name: String,
    pub size: u32,
    pub align: u32,
    pub is_function: bool,
}

// Intermediate declarator tree
#[derive(Debug)]
enum DeclNode {
    Name(String, String, u32), // name, file, line
    Abstract,
    Pointer { inner: Box<DeclNode>, is_const: bool, is_volatile: bool },
    Array { inner: Box<DeclNode>, size: Option<u64> },
    Function { inner: Box<DeclNode>, params: FuncParams },
}

fn apply_decl(node: DeclNode, base: CType) -> (Option<(String, String, u32)>, CType) {
    match node {
        DeclNode::Name(name, file, line) => (Some((name, file, line)), base),
        DeclNode::Abstract => (None, base),
        DeclNode::Pointer { inner, is_const, is_volatile } => {
            let ptr = CType {
                kind: TypeKind::Pointer(Box::new(base)),
                is_const,
                is_volatile,
            };
            apply_decl(*inner, ptr)
        }
        DeclNode::Array { inner, size } => {
            let arr = CType::new(TypeKind::Array { base: Box::new(base), size });
            apply_decl(*inner, arr)
        }
        DeclNode::Function { inner, params } => {
            let func = CType::new(TypeKind::Function {
                return_type: Box::new(base),
                params,
            });
            apply_decl(*inner, func)
        }
    }
}

// Declaration specifier accumulator
#[derive(Debug, Clone, PartialEq)]
enum StorageClassSpec {
    Auto,
    Static,
    Extern,
    Register,
    Typedef,
}

struct DeclSpecs {
    is_const: bool,
    is_volatile: bool,
    storage: Option<StorageClassSpec>,
    has_void: bool,
    has_char: bool,
    has_short: bool,
    long_count: u32,
    has_int: bool,
    has_float: bool,
    has_double: bool,
    has_signed: bool,
    has_unsigned: bool,
    struct_type: Option<TypeKind>,
    typedef_type: Option<CType>,
    has_type_keyword: bool,
}

impl DeclSpecs {
    fn new() -> Self {
        DeclSpecs {
            is_const: false, is_volatile: false, storage: None,
            has_void: false, has_char: false, has_short: false,
            long_count: 0, has_int: false, has_float: false,
            has_double: false, has_signed: false, has_unsigned: false,
            struct_type: None, typedef_type: None, has_type_keyword: false,
        }
    }
}

fn resolve_specs(specs: &DeclSpecs) -> CType {
    if let Some(ref td) = specs.typedef_type {
        let mut t = td.clone();
        if specs.is_const { t.is_const = true; }
        if specs.is_volatile { t.is_volatile = true; }
        return t;
    }

    let signedness = if specs.has_unsigned { Signedness::Unsigned }
                    else if specs.has_signed { Signedness::Signed }
                    else { Signedness::Default };

    let kind = if let Some(ref sk) = specs.struct_type {
        sk.clone()
    } else if specs.has_void {
        TypeKind::Void
    } else if specs.has_char {
        TypeKind::Char(signedness)
    } else if specs.has_short {
        TypeKind::Short(signedness)
    } else if specs.has_float {
        TypeKind::Float
    } else if specs.has_double {
        if specs.long_count > 0 { TypeKind::LongDouble } else { TypeKind::Double }
    } else if specs.long_count >= 2 {
        TypeKind::LongLong(signedness)
    } else if specs.long_count == 1 {
        TypeKind::Long(signedness)
    } else {
        TypeKind::Int(signedness)
    };

    CType { kind, is_const: specs.is_const, is_volatile: specs.is_volatile }
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    pub symtab: SymbolTable,
    pub struct_defs: Vec<StructDef>,
    fn_counter: u32,
    pub debug: bool,
    pub functions: Vec<FunctionQuads>,
    pub globals: Vec<GlobalDecl>,
    pub strings: Vec<String>,
    /// While parsing a function body, accumulator of all locals (including
    /// block-scoped) so the codegen can give each one a stack slot.
    current_locals: Option<Vec<crate::quad::LocalSymInfo>>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens, pos: 0,
            symtab: SymbolTable::new(),
            struct_defs: Vec::new(),
            fn_counter: 0,
            debug: false,
            functions: Vec::new(),
            globals: Vec::new(),
            strings: Vec::new(),
            current_locals: None,
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek(&self) -> &TokenKind {
        self.tokens.get(self.pos).map(|t| &t.kind).unwrap_or(&TokenKind::Eof)
    }

    fn peek_token(&self) -> &Token {
        static EOF_TOKEN: std::sync::LazyLock<Token> = std::sync::LazyLock::new(|| Token {
            kind: TokenKind::Eof,
            filename: String::new(),
            line: 0,
        });
        self.tokens.get(self.pos).unwrap_or(&EOF_TOKEN)
    }

    fn peek_at(&self, offset: usize) -> &TokenKind {
        self.tokens.get(self.pos + offset).map(|t| &t.kind).unwrap_or(&TokenKind::Eof)
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        self.pos += 1;
        tok
    }

    fn expect_char(&mut self, c: char) {
        if matches!(self.peek(), TokenKind::Char(ch) if *ch == c) {
            self.advance();
        } else {
            let tok = self.peek_token();
            eprintln!("Warning: Expected '{}', got {:?} at {}:{}", c, self.peek(), tok.filename, tok.line);
        }
    }

    // ---- Main entry ----

    pub fn parse_translation_unit(&mut self) {
        if let Some(tok) = self.tokens.first() {
            self.symtab.scopes[0].start_file = tok.filename.clone();
            self.symtab.scopes[0].start_line = 1;
        }

        while !self.at_end() {
            if matches!(self.peek(), TokenKind::Char(';')) {
                self.advance();
                continue;
            }
            self.parse_external_declaration();
        }
    }

    // ---- Declaration parsing ----

    fn parse_external_declaration(&mut self) {
        let specs = self.parse_declaration_specifiers();

        // Bare struct/union definition followed by ';'
        if matches!(self.peek(), TokenKind::Char(';')) {
            if let Some(TypeKind::StructRef { is_union, ref tag, def_id }) = specs.struct_type {
                if tag.is_some() {
                    let tag_name = tag.as_ref().unwrap();
                    if def_id.is_some() {
                        let in_current = self.symtab.lookup_tag_current(tag_name).is_some();
                        if !in_current {
                            self.symtab.insert_tag(TagEntry {
                                tag: tag_name.clone(),
                                is_union,
                                def_id: None,
                            });
                        }
                    }
                }
            }
            self.advance();
            return;
        }

        // Parse first declarator
        let decl = self.parse_declarator(true);
        let base = resolve_specs(&specs);
        let (name_info, ctype) = apply_decl(decl, base);

        if let Some((name, file, line)) = name_info {
            // Check if function definition
            if matches!(&ctype.kind, TypeKind::Function { .. }) {
                let is_knr = matches!(&ctype.kind, TypeKind::Function { params: FuncParams::KnR(_), .. });
                if matches!(self.peek(), TokenKind::Char('{')) || (is_knr && self.is_decl_spec_start()) {
                    self.handle_function_definition(name, file, line, ctype, &specs);
                    return;
                }
                if matches!(&ctype.kind, TypeKind::Function { params: FuncParams::Unknown, .. }) && self.is_decl_spec_start() {
                    self.handle_function_definition(name, file, line, ctype, &specs);
                    return;
                }
            }

            self.install_and_print_decl(&name, &file, line, &ctype, &specs);

            while matches!(self.peek(), TokenKind::Char(',')) {
                self.advance();
                let decl = self.parse_declarator(true);
                let base = resolve_specs(&specs);
                let (name_info, ctype) = apply_decl(decl, base);
                if let Some((name, file, line)) = name_info {
                    self.install_and_print_decl(&name, &file, line, &ctype, &specs);
                }
            }
        }

        if matches!(self.peek(), TokenKind::Char(';')) {
            self.advance();
        }
    }

    fn install_and_print_decl(&mut self, name: &str, file: &str, line: u32,
                               ctype: &CType, specs: &DeclSpecs) {
        let is_typedef = specs.storage == Some(StorageClassSpec::Typedef);
        let stg = self.resolve_storage_class(specs, ctype);

        let sym = Symbol {
            name: name.to_string(),
            ctype: ctype.clone(),
            storage_class: stg.clone(),
            is_typedef,
            file: file.to_string(),
            line,
            arg_num: None,
        };

        if self.debug {
            let scope = self.symtab.current_scope();
            if is_typedef {
                printer::print_typedef(name, file, line, ctype, scope, &self.struct_defs);
            } else if matches!(&ctype.kind, TypeKind::Function { .. }) {
                printer::print_function(name, file, line, ctype, &sym.storage_class,
                                       scope, &self.struct_defs);
            } else {
                printer::print_variable(name, file, line, ctype, &sym.storage_class,
                                       None, scope, &self.struct_defs);
            }
        }

        // Collect global variable declarations for codegen (.comm directives)
        if !is_typedef && self.symtab.is_global_scope() {
            let is_fn = matches!(&ctype.kind, TypeKind::Function { .. });
            if !is_fn {
                let size = type_size(ctype, &self.struct_defs).max(1);
                let align = type_align(ctype, &self.struct_defs).max(1);
                let already = self.globals.iter().any(|g| g.name == name);
                if !already {
                    self.globals.push(GlobalDecl {
                        name: name.to_string(),
                        size,
                        align,
                        is_function: false,
                    });
                }
            }
        } else if !is_typedef
            && !matches!(&ctype.kind, TypeKind::Function { .. })
            && stg != StorageClass::Extern
        {
            if let Some(ref mut locals) = self.current_locals {
                let size = type_size(ctype, &self.struct_defs).max(4);
                locals.push(crate::quad::LocalSymInfo {
                    name: name.to_string(),
                    size,
                });
            }
        }

        self.symtab.insert_symbol(sym);
    }

    fn resolve_storage_class(&self, specs: &DeclSpecs, ctype: &CType) -> StorageClass {
        match &specs.storage {
            Some(StorageClassSpec::Static) => StorageClass::Static,
            Some(StorageClassSpec::Extern) => StorageClass::Extern,
            Some(StorageClassSpec::Register) => StorageClass::Register,
            Some(StorageClassSpec::Auto) => StorageClass::Auto,
            Some(StorageClassSpec::Typedef) => StorageClass::Extern,
            None => {
                if self.symtab.is_global_scope() || matches!(&ctype.kind, TypeKind::Function { .. }) {
                    StorageClass::Extern
                } else {
                    StorageClass::Auto
                }
            }
        }
    }

    // ---- Function definition ----

    fn handle_function_definition(&mut self, name: String, _file: String, _line: u32,
                                   ctype: CType, specs: &DeclSpecs) {
        let stg = if specs.storage == Some(StorageClassSpec::Static) {
            StorageClass::Static
        } else {
            StorageClass::Extern
        };

        let (file, line) = {
            let tok = self.peek_token();
            (tok.filename.clone(), tok.line)
        };

        let mut ctype = ctype;
        if specs.has_type_keyword || specs.typedef_type.is_some() || specs.struct_type.is_some() {
            if let TypeKind::Function { params: FuncParams::Unknown, return_type } = ctype.kind {
                ctype = CType {
                    kind: TypeKind::Function { params: FuncParams::Void, return_type },
                    is_const: ctype.is_const,
                    is_volatile: ctype.is_volatile,
                };
            }
        }

        if self.debug {
            let scope = self.symtab.current_scope();
            printer::print_function(&name, &file, line, &ctype, &stg, scope, &self.struct_defs);
        }

        self.symtab.insert_symbol(Symbol {
            name: name.clone(),
            ctype: ctype.clone(),
            storage_class: stg,
            is_typedef: false,
            file: file.clone(),
            line,
            arg_num: None,
        });

        let params = match &ctype.kind {
            TypeKind::Function { params, .. } => params.clone(),
            _ => FuncParams::Unknown,
        };

        let knr_names: Option<Vec<String>> = match &params {
            FuncParams::KnR(names) => Some(names.clone()),
            _ => None,
        };

        let scope_file;
        let scope_line;
        if knr_names.is_some() || matches!(&params, FuncParams::Unknown) {
            scope_file = file.clone();
            scope_line = line;
        } else {
            let tok = self.peek_token();
            if matches!(self.peek(), TokenKind::Char('{')) {
                scope_file = tok.filename.clone();
                scope_line = tok.line;
            } else {
                scope_file = file.clone();
                scope_line = line;
            }
        }

        self.symtab.push_scope(ScopeKind::Function, scope_file, scope_line);

        if let FuncParams::Params(ref param_infos) = params {
            let brace_tok = self.peek_token();
            let brace_file = brace_tok.filename.clone();
            let brace_line = brace_tok.line;
            for (i, pi) in param_infos.iter().enumerate() {
                if let Some(ref pname) = pi.name {
                    let sym = Symbol {
                        name: pname.clone(),
                        ctype: pi.ctype.clone(),
                        storage_class: StorageClass::Auto,
                        is_typedef: false,
                        file: brace_file.clone(),
                        line: brace_line,
                        arg_num: Some((i + 1) as u32),
                    };
                    if self.debug {
                        let scope = self.symtab.current_scope();
                        printer::print_variable(pname, &brace_file, brace_line, &pi.ctype,
                                              &StorageClass::Auto, Some((i + 1) as u32),
                                              scope, &self.struct_defs);
                    }
                    self.symtab.insert_symbol(sym);
                }
            }
        }

        if let Some(ref knames) = knr_names {
            while !matches!(self.peek(), TokenKind::Char('{')) && !self.at_end() {
                let pspecs = self.parse_declaration_specifiers();
                loop {
                    let decl = self.parse_declarator(true);
                    let base = resolve_specs(&pspecs);
                    let (ni, ct) = apply_decl(decl, base);
                    if let Some((pn, pf, pl)) = ni {
                        let sym = Symbol {
                            name: pn.clone(),
                            ctype: ct.clone(),
                            storage_class: StorageClass::Auto,
                            is_typedef: false,
                            file: pf.clone(),
                            line: pl,
                            arg_num: None,
                        };
                        if self.debug {
                            let scope = self.symtab.current_scope();
                            printer::print_variable(&pn, &pf, pl, &ct,
                                                  &StorageClass::Auto, None,
                                                  scope, &self.struct_defs);
                        }
                        self.symtab.insert_symbol(sym);
                    }
                    if matches!(self.peek(), TokenKind::Char(',')) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                if matches!(self.peek(), TokenKind::Char(';')) {
                    self.advance();
                }
            }

            let brace_tok = self.peek_token();
            let brace_file = brace_tok.filename.clone();
            let brace_line = brace_tok.line;
            for (i, kn) in knames.iter().enumerate() {
                let arg_num = (i + 1) as u32;
                if self.symtab.lookup_symbol_current_scope(kn).is_some() {
                    self.symtab.set_arg_num(kn, arg_num);
                } else {
                    self.symtab.insert_symbol(Symbol {
                        name: kn.clone(),
                        ctype: CType::new(TypeKind::Int(Signedness::Default)),
                        storage_class: StorageClass::Auto,
                        is_typedef: false,
                        file: brace_file.clone(),
                        line: brace_line,
                        arg_num: Some(arg_num),
                    });
                }
            }
        }

        // Begin accumulating block-scoped locals for stack frame layout
        self.current_locals = Some(Vec::new());

        // Parse function body
        self.expect_char('{');
        let body = self.parse_compound_body();

        // Locals come from the accumulator populated during parsing
        let locals_info = self.current_locals.take().unwrap_or_default();

        self.fn_counter += 1;
        let mut qg = crate::quadgen::QuadGen::new(self.fn_counter, &self.struct_defs, &mut self.strings);
        qg.gen_function_body(&body);
        let fq = qg.into_function(name.clone(), locals_info);

        self.symtab.pop_scope();

        if self.debug {
            // Print AST dump and quads
            println!("AST Dump for function");
            crate::ast::print_ast(&body, 1);
            crate::quad::print_function_quads(&fq);
        }

        self.functions.push(fq);
    }

    // ---- Compound statement / statement parsing ----

    fn parse_compound_body(&mut self) -> AstNode {
        let mut stmts = Vec::new();
        while !matches!(self.peek(), TokenKind::Char('}')) && !self.at_end() {
            if self.is_whatis() {
                self.handle_whatis();
            } else if self.is_decl_spec_start() {
                let inits = self.parse_declaration_in_scope();
                stmts.extend(inits);
            } else {
                let stmt = self.parse_statement();
                if !matches!(stmt, AstNode::Noop) {
                    stmts.push(stmt);
                }
            }
        }
        if matches!(self.peek(), TokenKind::Char('}')) {
            self.advance();
        }
        AstNode::List(stmts)
    }

    fn parse_statement(&mut self) -> AstNode {
        match self.peek().clone() {
            TokenKind::Char('{') => {
                self.advance();
                let tok = self.peek_token();
                let bf = tok.filename.clone();
                let bl = tok.line;
                self.symtab.push_scope(ScopeKind::Block, bf, bl);
                let list = self.parse_compound_body();
                self.symtab.pop_scope();
                list
            }
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::Do => self.parse_do_while(),
            TokenKind::For => self.parse_for(),
            TokenKind::Switch => self.parse_switch(),
            TokenKind::Case => self.parse_case(),
            TokenKind::Default => self.parse_default(),
            TokenKind::Break => {
                self.advance();
                self.expect_char(';');
                AstNode::Break
            }
            TokenKind::Continue => {
                self.advance();
                self.expect_char(';');
                AstNode::Continue
            }
            TokenKind::Return => self.parse_return(),
            TokenKind::Goto => self.parse_goto(),
            TokenKind::Char(';') => {
                self.advance();
                AstNode::Noop
            }
            TokenKind::Ident(_) if self.is_label() => self.parse_label(),
            _ => {
                let expr = self.parse_expr();
                self.expect_char(';');
                expr
            }
        }
    }

    fn parse_if(&mut self) -> AstNode {
        self.advance(); // consume 'if'
        self.expect_char('(');
        let cond = self.parse_expr();
        self.expect_char(')');
        let then_body = self.parse_statement();
        let else_body = if matches!(self.peek(), TokenKind::Else) {
            self.advance();
            Some(Box::new(self.parse_statement()))
        } else {
            None
        };
        AstNode::If {
            cond: Box::new(cond),
            then_body: Box::new(then_body),
            else_body,
        }
    }

    fn parse_while(&mut self) -> AstNode {
        self.advance(); // consume 'while'
        self.expect_char('(');
        let cond = self.parse_expr();
        self.expect_char(')');
        let body = self.parse_statement();
        AstNode::While {
            cond: Box::new(cond),
            body: Box::new(body),
        }
    }

    fn parse_do_while(&mut self) -> AstNode {
        self.advance(); // consume 'do'
        let body = self.parse_statement();
        if matches!(self.peek(), TokenKind::While) {
            self.advance();
        }
        self.expect_char('(');
        let cond = self.parse_expr();
        self.expect_char(')');
        self.expect_char(';');
        AstNode::DoWhile {
            body: Box::new(body),
            cond: Box::new(cond),
        }
    }

    fn parse_for(&mut self) -> AstNode {
        self.advance(); // consume 'for'
        self.expect_char('(');

        let mut stealth_scope = false;

        // Init clause
        let init = if matches!(self.peek(), TokenKind::Char(';')) {
            self.advance();
            None
        } else if self.is_decl_spec_start() {
            // C99 for-declaration: push stealth block scope
            stealth_scope = true;
            let tok = self.peek_token();
            self.symtab.push_scope(ScopeKind::Block, tok.filename.clone(), tok.line);
            let inits = self.parse_declaration_in_scope();
            if inits.len() == 1 {
                Some(Box::new(inits.into_iter().next().unwrap()))
            } else if inits.is_empty() {
                None
            } else {
                Some(Box::new(AstNode::List(inits)))
            }
        } else {
            let e = self.parse_expr();
            self.expect_char(';');
            Some(Box::new(e))
        };

        // Condition
        let cond = if matches!(self.peek(), TokenKind::Char(';')) {
            self.advance();
            None
        } else {
            let e = self.parse_expr();
            self.expect_char(';');
            Some(Box::new(e))
        };

        // Increment
        let incr = if matches!(self.peek(), TokenKind::Char(')')) {
            None
        } else {
            Some(Box::new(self.parse_expr()))
        };

        self.expect_char(')');

        let body = self.parse_statement();

        if stealth_scope {
            self.symtab.pop_scope();
        }

        AstNode::For {
            init,
            cond,
            incr,
            body: Box::new(body),
        }
    }

    fn parse_switch(&mut self) -> AstNode {
        self.advance(); // consume 'switch'
        self.expect_char('(');
        let expr = self.parse_expr();
        self.expect_char(')');
        let body = self.parse_statement();
        AstNode::Switch {
            expr: Box::new(expr),
            body: Box::new(body),
        }
    }

    fn parse_case(&mut self) -> AstNode {
        self.advance(); // consume 'case'
        let expr = self.parse_expr();
        self.expect_char(':');
        let stmt = self.parse_statement();
        AstNode::Case {
            expr: Box::new(expr),
            stmt: Box::new(stmt),
        }
    }

    fn parse_default(&mut self) -> AstNode {
        self.advance(); // consume 'default'
        self.expect_char(':');
        let stmt = self.parse_statement();
        AstNode::Default(Box::new(stmt))
    }

    fn parse_return(&mut self) -> AstNode {
        self.advance(); // consume 'return'
        if matches!(self.peek(), TokenKind::Char(';')) {
            self.advance();
            AstNode::Return(None)
        } else {
            let e = self.parse_expr();
            self.expect_char(';');
            AstNode::Return(Some(Box::new(e)))
        }
    }

    fn parse_goto(&mut self) -> AstNode {
        self.advance(); // consume 'goto'
        let label = if let TokenKind::Ident(ref name) = self.peek().clone() {
            let name = name.clone();
            self.advance();
            name
        } else {
            "?".to_string()
        };
        self.expect_char(';');
        AstNode::Goto(label)
    }

    fn is_label(&self) -> bool {
        matches!(self.peek(), TokenKind::Ident(_)) && matches!(self.peek_at(1), TokenKind::Char(':'))
    }

    fn parse_label(&mut self) -> AstNode {
        let name = if let TokenKind::Ident(ref n) = self.peek().clone() {
            let n = n.clone();
            self.advance();
            n
        } else {
            unreachable!()
        };
        self.expect_char(':');
        let stmt = self.parse_statement();
        AstNode::Label {
            name,
            stmt: Box::new(stmt),
        }
    }

    // ---- Declaration in scope (returns init AST nodes) ----

    fn parse_declaration_in_scope(&mut self) -> Vec<AstNode> {
        let specs = self.parse_declaration_specifiers();
        let mut inits = Vec::new();

        if matches!(self.peek(), TokenKind::Char(';')) {
            // Bare struct def or forward decl
            if let Some(TypeKind::StructRef { is_union, ref tag, def_id }) = specs.struct_type {
                if let Some(tag_name) = tag {
                    if def_id.is_some() {
                        let in_current = self.symtab.lookup_tag_current(tag_name).is_some();
                        if !in_current {
                            self.symtab.insert_tag(TagEntry {
                                tag: tag_name.clone(),
                                is_union,
                                def_id: None,
                            });
                        }
                    } else {
                        let in_current = self.symtab.lookup_tag_current(tag_name).is_some();
                        if !in_current {
                            self.symtab.insert_tag(TagEntry {
                                tag: tag_name.clone(),
                                is_union,
                                def_id: None,
                            });
                        }
                    }
                }
            }
            self.advance();
            return inits;
        }

        loop {
            let decl = self.parse_declarator(true);
            let base = resolve_specs(&specs);
            let (name_info, ctype) = apply_decl(decl, base);
            if let Some((name, file, line)) = name_info {
                self.install_and_print_decl(&name, &file, line, &ctype, &specs);

                // Check for initializer
                if matches!(self.peek(), TokenKind::Char('=')) {
                    self.advance();
                    let init_expr = self.parse_assignment_expr();
                    // Create assignment AST
                    let var_node = self.make_ident_node(&name);
                    inits.push(AstNode::Assignment {
                        left: Box::new(var_node),
                        right: Box::new(init_expr),
                    });
                }
            }
            if matches!(self.peek(), TokenKind::Char(',')) {
                self.advance();
            } else {
                break;
            }
        }
        if matches!(self.peek(), TokenKind::Char(';')) {
            self.advance();
        }
        inits
    }

    fn make_ident_node(&self, name: &str) -> AstNode {
        if let Some((scope_idx, sym)) = self.symtab.lookup_with_scope(name) {
            let file = sym.file.clone();
            let line = sym.line;
            if matches!(&sym.ctype.kind, TypeKind::Function { .. }) {
                AstNode::StabFn { name: name.to_string(), file, line, ctype: Some(sym.ctype.clone()) }
            } else {
                let class = classify_sym(scope_idx, sym);
                AstNode::StabVar { name: name.to_string(), file, line, class, ctype: Some(sym.ctype.clone()) }
            }
        } else {
            AstNode::StabVar {
                name: name.to_string(), file: String::new(), line: 0,
                class: crate::quad::SymClass::Global, ctype: None,
            }
        }
    }

    // ---- Whatis (kept from assign3) ----

    fn is_whatis(&self) -> bool {
        matches!(self.peek(), TokenKind::Ident(ref s) if s == "_whatis")
    }

    fn handle_whatis(&mut self) {
        self.advance();
        if let TokenKind::Ident(ref name) = self.peek().clone() {
            let name = name.clone();
            self.advance();
            if let Some(sym_idx) = self.find_symbol_with_scope(&name) {
                let (sym, scope_idx) = sym_idx;
                let sym_clone = Symbol {
                    name: sym.name.clone(),
                    ctype: sym.ctype.clone(),
                    storage_class: sym.storage_class.clone(),
                    is_typedef: sym.is_typedef,
                    file: sym.file.clone(),
                    line: sym.line,
                    arg_num: sym.arg_num,
                };
                if self.debug {
                    let scope = &self.symtab.scopes[scope_idx];
                    print!("You asked about {}, ", name);
                    printer::print_symbol_info(&sym_clone, scope, &self.struct_defs);
                }
            }
        }
        if matches!(self.peek(), TokenKind::Char(';')) {
            self.advance();
        }
    }

    fn find_symbol_with_scope(&self, name: &str) -> Option<(&Symbol, usize)> {
        for (i, scope) in self.symtab.scopes.iter().enumerate().rev() {
            for sym in scope.ordinary.iter().rev() {
                if sym.name == name {
                    return Some((sym, i));
                }
            }
        }
        None
    }

    // ======== Expression parsing (from assign2, modified for symtab) ========

    fn parse_expr(&mut self) -> AstNode {
        self.parse_comma_expr()
    }

    fn parse_comma_expr(&mut self) -> AstNode {
        let mut left = self.parse_assignment_expr();
        while matches!(self.peek(), TokenKind::Char(',')) {
            self.advance();
            let right = self.parse_assignment_expr();
            left = AstNode::Comma {
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_assignment_expr(&mut self) -> AstNode {
        let left = self.parse_ternary();
        match self.peek() {
            TokenKind::Char('=') => {
                self.advance();
                let right = self.parse_assignment_expr();
                AstNode::Assignment {
                    left: Box::new(left),
                    right: Box::new(right),
                }
            }
            TokenKind::Pluseq | TokenKind::Minuseq | TokenKind::Timeseq |
            TokenKind::Diveq | TokenKind::Modeq | TokenKind::Shleq |
            TokenKind::Shreq | TokenKind::Andeq | TokenKind::Oreq |
            TokenKind::Xoreq => {
                let op = match self.peek() {
                    TokenKind::Pluseq => "+",
                    TokenKind::Minuseq => "-",
                    TokenKind::Timeseq => "*",
                    TokenKind::Diveq => "/",
                    TokenKind::Modeq => "%",
                    TokenKind::Shleq => "<<",
                    TokenKind::Shreq => ">>",
                    TokenKind::Andeq => "&",
                    TokenKind::Oreq => "|",
                    TokenKind::Xoreq => "^",
                    _ => unreachable!(),
                }.to_string();
                self.advance();
                let right = self.parse_assignment_expr();
                AstNode::Assignment {
                    left: Box::new(left.clone()),
                    right: Box::new(AstNode::BinaryOp {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    }),
                }
            }
            _ => left,
        }
    }

    fn parse_ternary(&mut self) -> AstNode {
        let cond = self.parse_logical_or();
        if matches!(self.peek(), TokenKind::Char('?')) {
            self.advance();
            let then_expr = self.parse_expr();
            self.expect_char(':');
            let else_expr = self.parse_ternary();
            AstNode::Ternary {
                cond: Box::new(cond),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            }
        } else {
            cond
        }
    }

    fn parse_logical_or(&mut self) -> AstNode {
        let mut left = self.parse_logical_and();
        while matches!(self.peek(), TokenKind::Logor) {
            self.advance();
            let right = self.parse_logical_and();
            left = AstNode::LogicalOp {
                op: "||".to_string(),
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_logical_and(&mut self) -> AstNode {
        let mut left = self.parse_bitwise_or();
        while matches!(self.peek(), TokenKind::Logand) {
            self.advance();
            let right = self.parse_bitwise_or();
            left = AstNode::LogicalOp {
                op: "&&".to_string(),
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_bitwise_or(&mut self) -> AstNode {
        let mut left = self.parse_bitwise_xor();
        while matches!(self.peek(), TokenKind::Char('|')) {
            self.advance();
            let right = self.parse_bitwise_xor();
            left = AstNode::BinaryOp {
                op: "|".to_string(),
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_bitwise_xor(&mut self) -> AstNode {
        let mut left = self.parse_bitwise_and();
        while matches!(self.peek(), TokenKind::Char('^')) {
            self.advance();
            let right = self.parse_bitwise_and();
            left = AstNode::BinaryOp {
                op: "^".to_string(),
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_bitwise_and(&mut self) -> AstNode {
        let mut left = self.parse_equality();
        while matches!(self.peek(), TokenKind::Char('&')) {
            self.advance();
            let right = self.parse_equality();
            left = AstNode::BinaryOp {
                op: "&".to_string(),
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_equality(&mut self) -> AstNode {
        let mut left = self.parse_relational();
        loop {
            let op = match self.peek() {
                TokenKind::Eqeq => "==",
                TokenKind::Noteq => "!=",
                _ => break,
            };
            let op = op.to_string();
            self.advance();
            let right = self.parse_relational();
            left = AstNode::ComparisonOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_relational(&mut self) -> AstNode {
        let mut left = self.parse_shift();
        loop {
            let op = match self.peek() {
                TokenKind::Char('<') => "<",
                TokenKind::Char('>') => ">",
                TokenKind::Lteq => "<=",
                TokenKind::Gteq => ">=",
                _ => break,
            };
            let op = op.to_string();
            self.advance();
            let right = self.parse_shift();
            left = AstNode::ComparisonOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_shift(&mut self) -> AstNode {
        let mut left = self.parse_additive();
        loop {
            let op = match self.peek() {
                TokenKind::Shl => "<<",
                TokenKind::Shr => ">>",
                _ => break,
            };
            let op = op.to_string();
            self.advance();
            let right = self.parse_additive();
            left = AstNode::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_additive(&mut self) -> AstNode {
        let mut left = self.parse_multiplicative();
        loop {
            let op = match self.peek() {
                TokenKind::Char('+') => "+",
                TokenKind::Char('-') => "-",
                _ => break,
            };
            let op = op.to_string();
            self.advance();
            let right = self.parse_multiplicative();
            left = AstNode::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_multiplicative(&mut self) -> AstNode {
        let mut left = self.parse_unary();
        loop {
            let op = match self.peek() {
                TokenKind::Char('*') => "*",
                TokenKind::Char('/') => "/",
                TokenKind::Char('%') => "%",
                _ => break,
            };
            let op = op.to_string();
            self.advance();
            let right = self.parse_unary();
            left = AstNode::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_unary(&mut self) -> AstNode {
        match self.peek().clone() {
            TokenKind::Char('!') => {
                self.advance();
                let operand = self.parse_unary();
                AstNode::UnaryOp { op: "!".to_string(), operand: Box::new(operand) }
            }
            TokenKind::Char('~') => {
                self.advance();
                let operand = self.parse_unary();
                AstNode::UnaryOp { op: "~".to_string(), operand: Box::new(operand) }
            }
            TokenKind::Plusplus => {
                self.advance();
                let operand = self.parse_unary();
                let one = AstNode::Number { numtype: "int".to_string(), val: "1".to_string() };
                AstNode::Assignment {
                    left: Box::new(operand.clone()),
                    right: Box::new(AstNode::BinaryOp {
                        op: "+".to_string(),
                        left: Box::new(operand),
                        right: Box::new(one),
                    }),
                }
            }
            TokenKind::Minusminus => {
                self.advance();
                let operand = self.parse_unary();
                let one = AstNode::Number { numtype: "int".to_string(), val: "1".to_string() };
                AstNode::Assignment {
                    left: Box::new(operand.clone()),
                    right: Box::new(AstNode::BinaryOp {
                        op: "-".to_string(),
                        left: Box::new(operand),
                        right: Box::new(one),
                    }),
                }
            }
            TokenKind::Char('+') => {
                self.advance();
                let operand = self.parse_unary();
                AstNode::UnaryOp { op: "+".to_string(), operand: Box::new(operand) }
            }
            TokenKind::Char('-') => {
                self.advance();
                let operand = self.parse_unary();
                AstNode::UnaryOp { op: "-".to_string(), operand: Box::new(operand) }
            }
            TokenKind::Char('*') => {
                self.advance();
                let operand = self.parse_unary();
                AstNode::Deref(Box::new(operand))
            }
            TokenKind::Char('&') => {
                self.advance();
                let operand = self.parse_unary();
                AstNode::AddressOf(Box::new(operand))
            }
            TokenKind::Sizeof => {
                self.advance();
                // sizeof(expr) or sizeof expr
                let operand = self.parse_unary();
                AstNode::Sizeof(Box::new(operand))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> AstNode {
        let mut node = self.parse_primary();
        loop {
            match self.peek() {
                TokenKind::Char('(') => {
                    self.advance();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), TokenKind::Char(')')) {
                        args.push(self.parse_assignment_expr());
                        while matches!(self.peek(), TokenKind::Char(',')) {
                            self.advance();
                            args.push(self.parse_assignment_expr());
                        }
                    }
                    self.expect_char(')');
                    node = AstNode::FnCall {
                        func: Box::new(node),
                        args,
                    };
                }
                TokenKind::Char('[') => {
                    self.advance();
                    let index = self.parse_expr();
                    self.expect_char(']');
                    node = AstNode::Deref(Box::new(AstNode::BinaryOp {
                        op: "+".to_string(),
                        left: Box::new(node),
                        right: Box::new(index),
                    }));
                }
                TokenKind::Char('.') => {
                    self.advance();
                    let member = self.expect_ident();
                    node = AstNode::DirectSelect {
                        obj: Box::new(node),
                        member,
                    };
                }
                TokenKind::Indsel => {
                    self.advance();
                    let member = self.expect_ident();
                    node = AstNode::IndirectSelect {
                        obj: Box::new(node),
                        member,
                    };
                }
                TokenKind::Plusplus => {
                    self.advance();
                    node = AstNode::UnaryOp {
                        op: "POSTINC".to_string(),
                        operand: Box::new(node),
                    };
                }
                TokenKind::Minusminus => {
                    self.advance();
                    node = AstNode::UnaryOp {
                        op: "POSTDEC".to_string(),
                        operand: Box::new(node),
                    };
                }
                _ => break,
            }
        }
        node
    }

    fn parse_primary(&mut self) -> AstNode {
        match self.peek().clone() {
            TokenKind::Ident(ref name) if name == "_whatis" => {
                // Skip _whatis in expression context
                self.advance();
                AstNode::Noop
            }
            TokenKind::Ident(ref name) => {
                let name = name.clone();
                self.advance();
                if let Some((scope_idx, sym)) = self.symtab.lookup_with_scope(&name) {
                    let file = sym.file.clone();
                    let line = sym.line;
                    if matches!(&sym.ctype.kind, TypeKind::Function { .. }) {
                        AstNode::StabFn { name, file, line, ctype: Some(sym.ctype.clone()) }
                    } else {
                        let class = classify_sym(scope_idx, sym);
                        AstNode::StabVar { name, file, line, class, ctype: Some(sym.ctype.clone()) }
                    }
                } else {
                    if matches!(self.peek(), TokenKind::Char('(')) {
                        let tok = self.peek_token();
                        let file = tok.filename.clone();
                        let line = tok.line;
                        let fn_ctype = CType::new(TypeKind::Function {
                            return_type: Box::new(CType::new(TypeKind::Int(Signedness::Default))),
                            params: FuncParams::Unknown,
                        });
                        self.symtab.insert_symbol_global(Symbol {
                            name: name.clone(),
                            ctype: fn_ctype.clone(),
                            storage_class: StorageClass::Extern,
                            is_typedef: false,
                            file: file.clone(),
                            line,
                            arg_num: None,
                        });
                        AstNode::StabFn { name, file, line, ctype: Some(fn_ctype) }
                    } else {
                        let tok = self.peek_token();
                        AstNode::StabVar {
                            name, file: tok.filename.clone(), line: tok.line,
                            class: SymClass::Global, ctype: None,
                        }
                    }
                }
            }
            TokenKind::Number(ref nv) => {
                let nv = nv.clone();
                self.advance();
                let numtype = number_type_str(&nv);
                let val = number_val_str(&nv);
                AstNode::Number { numtype, val }
            }
            TokenKind::Str(ref bytes) => {
                let bytes = bytes.clone();
                self.advance();
                // Concatenate adjacent string literals
                let mut all_bytes = bytes;
                while let TokenKind::Str(ref more) = self.peek().clone() {
                    let more = more.clone();
                    self.advance();
                    all_bytes.extend(more);
                }
                let s = String::from_utf8_lossy(&all_bytes).to_string();
                AstNode::Str(s)
            }
            TokenKind::CharLit(ref bytes) => {
                let bytes = bytes.clone();
                self.advance();
                let val = if bytes.is_empty() { 0u64 } else { bytes[0] as u64 };
                AstNode::Number {
                    numtype: "int".to_string(),
                    val: val.to_string(),
                }
            }
            TokenKind::Char('(') => {
                self.advance();
                let expr = self.parse_expr();
                self.expect_char(')');
                expr
            }
            other => {
                eprintln!("Warning: Unexpected token in expression: {:?}", other);
                self.advance();
                AstNode::Noop
            }
        }
    }

    fn expect_ident(&mut self) -> String {
        if let TokenKind::Ident(name) = self.peek().clone() {
            self.advance();
            name
        } else {
            eprintln!("Warning: Expected identifier, got {:?}", self.peek());
            "?".to_string()
        }
    }

    // ---- Declaration specifiers ----

    fn parse_declaration_specifiers(&mut self) -> DeclSpecs {
        let mut specs = DeclSpecs::new();

        loop {
            match self.peek().clone() {
                TokenKind::Auto => { self.advance(); specs.storage = Some(StorageClassSpec::Auto); }
                TokenKind::Static => { self.advance(); specs.storage = Some(StorageClassSpec::Static); }
                TokenKind::Extern => { self.advance(); specs.storage = Some(StorageClassSpec::Extern); }
                TokenKind::Register => { self.advance(); specs.storage = Some(StorageClassSpec::Register); }
                TokenKind::Typedef => { self.advance(); specs.storage = Some(StorageClassSpec::Typedef); }

                TokenKind::Const => { self.advance(); specs.is_const = true; }
                TokenKind::Volatile => { self.advance(); specs.is_volatile = true; }

                TokenKind::Void => { self.advance(); specs.has_void = true; specs.has_type_keyword = true; }
                TokenKind::KwChar => { self.advance(); specs.has_char = true; specs.has_type_keyword = true; }
                TokenKind::Short => { self.advance(); specs.has_short = true; specs.has_type_keyword = true; }
                TokenKind::Int => { self.advance(); specs.has_int = true; specs.has_type_keyword = true; }
                TokenKind::Long => { self.advance(); specs.long_count += 1; specs.has_type_keyword = true; }
                TokenKind::Float => { self.advance(); specs.has_float = true; specs.has_type_keyword = true; }
                TokenKind::Double => { self.advance(); specs.has_double = true; specs.has_type_keyword = true; }
                TokenKind::Signed => { self.advance(); specs.has_signed = true; specs.has_type_keyword = true; }
                TokenKind::Unsigned => { self.advance(); specs.has_unsigned = true; specs.has_type_keyword = true; }

                TokenKind::Struct => {
                    self.advance();
                    specs.struct_type = Some(self.parse_struct_or_union_spec(false));
                    specs.has_type_keyword = true;
                }
                TokenKind::Union => {
                    self.advance();
                    specs.struct_type = Some(self.parse_struct_or_union_spec(true));
                    specs.has_type_keyword = true;
                }

                TokenKind::Ident(ref name) if !specs.has_type_keyword && self.symtab.is_typedef_name(name) => {
                    let name = name.clone();
                    self.advance();
                    let sym = self.symtab.lookup_symbol(&name).unwrap();
                    specs.typedef_type = Some(sym.ctype.clone());
                    specs.has_type_keyword = true;
                }

                _ => break,
            }
        }

        specs
    }

    fn is_decl_spec_start(&self) -> bool {
        match self.peek() {
            TokenKind::Auto | TokenKind::Static | TokenKind::Extern |
            TokenKind::Register | TokenKind::Typedef |
            TokenKind::Const | TokenKind::Volatile |
            TokenKind::Void | TokenKind::KwChar | TokenKind::Short |
            TokenKind::Int | TokenKind::Long | TokenKind::Float |
            TokenKind::Double | TokenKind::Signed | TokenKind::Unsigned |
            TokenKind::Struct | TokenKind::Union => true,
            TokenKind::Ident(ref name) => self.symtab.is_typedef_name(name),
            _ => false,
        }
    }

    // ---- Struct/union specifier ----

    fn parse_struct_or_union_spec(&mut self, is_union: bool) -> TypeKind {
        let tag = if let TokenKind::Ident(ref name) = self.peek().clone() {
            let name = name.clone();
            self.advance();
            Some(name)
        } else {
            None
        };

        if matches!(self.peek(), TokenKind::Char('{')) {
            return self.parse_struct_body(is_union, tag);
        }

        if let Some(ref tag_name) = tag {
            if let Some(entry) = self.symtab.lookup_tag(tag_name) {
                return TypeKind::StructRef {
                    is_union: entry.is_union,
                    tag: Some(tag_name.clone()),
                    def_id: entry.def_id,
                };
            }
            self.symtab.insert_tag(TagEntry {
                tag: tag_name.clone(),
                is_union,
                def_id: None,
            });
            TypeKind::StructRef {
                is_union,
                tag: Some(tag_name.clone()),
                def_id: None,
            }
        } else {
            TypeKind::StructRef { is_union, tag: None, def_id: None }
        }
    }

    fn parse_struct_body(&mut self, is_union: bool, tag: Option<String>) -> TypeKind {
        let brace_tok = self.peek_token();
        let def_file = brace_tok.filename.clone();
        let def_line = brace_tok.line;
        self.advance(); // consume '{'

        self.symtab.push_scope(ScopeKind::StructUnion, def_file.clone(), def_line);

        let mut members = Vec::new();

        while !matches!(self.peek(), TokenKind::Char('}')) && !self.at_end() {
            let mspecs = self.parse_declaration_specifiers();

            if matches!(self.peek(), TokenKind::Char(';')) {
                self.advance();
                continue;
            }

            loop {
                let member_tok = self.peek_token();
                let mfile = member_tok.filename.clone();
                let mline = member_tok.line;

                let decl = self.parse_declarator(true);
                let base = resolve_specs(&mspecs);
                let (name_info, mctype) = apply_decl(decl, base);

                let bit_width = if matches!(self.peek(), TokenKind::Char(':')) {
                    self.advance();
                    if let TokenKind::Number(ref nv) = self.peek().clone() {
                        let w = nv.int_val as u32;
                        self.advance();
                        w
                    } else {
                        0
                    }
                } else {
                    0
                };

                let mname = name_info.map(|(n, f, l)| (n, f, l));
                let (name, file, line) = if let Some((n, f, l)) = mname {
                    (Some(n), f, l)
                } else {
                    (None, mfile, mline)
                };

                members.push(StructMember {
                    name,
                    ctype: mctype,
                    offset: 0,
                    bit_offset: 0,
                    bit_width,
                    file,
                    line,
                });

                if matches!(self.peek(), TokenKind::Char(',')) {
                    self.advance();
                } else {
                    break;
                }
            }

            if matches!(self.peek(), TokenKind::Char(';')) {
                self.advance();
            }
        }

        self.expect_char('}');

        let size = compute_struct_layout(&mut members, is_union, &self.struct_defs);

        let def_id = self.struct_defs.len();
        let def = StructDef {
            is_union,
            tag: tag.clone(),
            members,
            size,
            def_file: def_file.clone(),
            def_line,
        };
        self.struct_defs.push(def);

        if self.debug {
            let scope = self.symtab.current_scope();
            printer::print_struct_def(&self.struct_defs[def_id], scope, &self.struct_defs);
        }

        self.symtab.pop_scope();

        if let Some(ref tag_name) = tag {
            let existing = self.symtab.lookup_tag_current(tag_name);
            if let Some(entry) = existing {
                if entry.def_id.is_none() {
                    self.symtab.update_tag_def(tag_name, def_id);
                } else {
                    self.symtab.insert_tag(TagEntry {
                        tag: tag_name.clone(),
                        is_union,
                        def_id: Some(def_id),
                    });
                }
            } else {
                self.symtab.insert_tag(TagEntry {
                    tag: tag_name.clone(),
                    is_union,
                    def_id: Some(def_id),
                });
            }
        }

        TypeKind::StructRef {
            is_union,
            tag,
            def_id: Some(def_id),
        }
    }

    // ---- Declarator parsing ----

    fn parse_declarator(&mut self, allow_name: bool) -> DeclNode {
        if matches!(self.peek(), TokenKind::Char('*')) {
            self.advance();
            let mut is_const = false;
            let mut is_volatile = false;
            loop {
                match self.peek() {
                    TokenKind::Const => { self.advance(); is_const = true; }
                    TokenKind::Volatile => { self.advance(); is_volatile = true; }
                    _ => break,
                }
            }
            let inner = self.parse_declarator(allow_name);
            DeclNode::Pointer { inner: Box::new(inner), is_const, is_volatile }
        } else {
            self.parse_direct_declarator(allow_name)
        }
    }

    fn parse_direct_declarator(&mut self, allow_name: bool) -> DeclNode {
        let mut base = match self.peek().clone() {
            TokenKind::Ident(ref name) if allow_name && !self.symtab.is_typedef_name(name) => {
                let tok = self.peek_token();
                let name = name.clone();
                let file = tok.filename.clone();
                let line = tok.line;
                self.advance();
                DeclNode::Name(name, file, line)
            }
            TokenKind::Ident(ref name) if allow_name => {
                let tok = self.peek_token();
                let name = name.clone();
                let file = tok.filename.clone();
                let line = tok.line;
                self.advance();
                DeclNode::Name(name, file, line)
            }
            TokenKind::Char('(') if self.is_grouped_declarator(allow_name) => {
                self.advance();
                let inner = self.parse_declarator(allow_name);
                self.expect_char(')');
                inner
            }
            _ => DeclNode::Abstract,
        };

        loop {
            match self.peek() {
                TokenKind::Char('(') => {
                    let params = self.parse_param_list();
                    base = DeclNode::Function { inner: Box::new(base), params };
                }
                TokenKind::Char('[') => {
                    self.advance();
                    let size = if matches!(self.peek(), TokenKind::Char(']')) {
                        None
                    } else if let TokenKind::Number(ref nv) = self.peek().clone() {
                        let s = nv.int_val;
                        self.advance();
                        Some(s)
                    } else {
                        while !matches!(self.peek(), TokenKind::Char(']') | TokenKind::Eof) {
                            self.advance();
                        }
                        None
                    };
                    self.expect_char(']');
                    base = DeclNode::Array { inner: Box::new(base), size };
                }
                _ => break,
            }
        }

        base
    }

    fn is_grouped_declarator(&self, allow_name: bool) -> bool {
        match self.peek_at(1) {
            TokenKind::Char('*') => true,
            TokenKind::Char(')') => false,
            TokenKind::Void | TokenKind::KwChar | TokenKind::Short |
            TokenKind::Int | TokenKind::Long | TokenKind::Float |
            TokenKind::Double | TokenKind::Signed | TokenKind::Unsigned |
            TokenKind::Const | TokenKind::Volatile |
            TokenKind::Struct | TokenKind::Union |
            TokenKind::Ellipsis |
            TokenKind::Auto | TokenKind::Static | TokenKind::Extern |
            TokenKind::Register => false,
            TokenKind::Ident(ref name) => {
                if allow_name {
                    true
                } else if self.symtab.is_typedef_name(name) {
                    false
                } else {
                    true
                }
            }
            TokenKind::Char('(') => true,
            _ => false,
        }
    }

    fn parse_param_list(&mut self) -> FuncParams {
        self.advance(); // consume '('

        if matches!(self.peek(), TokenKind::Char(')')) {
            self.advance();
            return FuncParams::Unknown;
        }

        if matches!(self.peek(), TokenKind::Void) && matches!(self.peek_at(1), TokenKind::Char(')')) {
            self.advance();
            self.advance();
            return FuncParams::Void;
        }

        if self.looks_like_knr_params() {
            return self.parse_knr_param_names();
        }

        let mut params = Vec::new();
        loop {
            if matches!(self.peek(), TokenKind::Ellipsis) {
                self.advance();
                break;
            }

            let pspecs = self.parse_declaration_specifiers();
            let decl = self.parse_declarator(true);
            let base = resolve_specs(&pspecs);
            let (name_info, pctype) = apply_decl(decl, base);

            let pname = name_info.map(|(n, _, _)| n);

            params.push(ParamInfo {
                ctype: pctype,
                name: pname,
            });

            if matches!(self.peek(), TokenKind::Char(',')) {
                self.advance();
            } else {
                break;
            }
        }

        self.expect_char(')');
        FuncParams::Params(params)
    }

    fn looks_like_knr_params(&self) -> bool {
        let mut i = self.pos;
        loop {
            if i >= self.tokens.len() { return false; }
            match &self.tokens[i].kind {
                TokenKind::Char(')') => return true,
                TokenKind::Ident(name) => {
                    if self.symtab.is_typedef_name(name) {
                        return false;
                    }
                    i += 1;
                    if i >= self.tokens.len() { return false; }
                    match &self.tokens[i].kind {
                        TokenKind::Char(',') => { i += 1; }
                        TokenKind::Char(')') => return true,
                        _ => return false,
                    }
                }
                _ => return false,
            }
        }
    }

    fn parse_knr_param_names(&mut self) -> FuncParams {
        let mut names = Vec::new();
        loop {
            if let TokenKind::Ident(ref name) = self.peek().clone() {
                names.push(name.clone());
                self.advance();
            }
            if matches!(self.peek(), TokenKind::Char(',')) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect_char(')');
        FuncParams::KnR(names)
    }
}

// Struct layout computation

fn compute_struct_layout(members: &mut [StructMember], is_union: bool,
                         struct_defs: &[StructDef]) -> u32 {
    if is_union {
        let mut max_size = 0u32;
        for m in members.iter_mut() {
            m.offset = 0;
            m.bit_offset = 0;
            let sz = type_size(&m.ctype, struct_defs);
            if sz > max_size { max_size = sz; }
        }
        let max_align = members.iter()
            .map(|m| type_align(&m.ctype, struct_defs))
            .max().unwrap_or(1);
        align_to(max_size, max_align)
    } else {
        let mut offset = 0u32;
        let mut bit_pos = 0u32;
        let mut unit_size = 0u32;

        for m in members.iter_mut() {
            if m.bit_width > 0 {
                let type_sz = type_size(&m.ctype, struct_defs);
                let unit_bits = type_sz * 8;

                if unit_size == 0 || bit_pos + m.bit_width > unit_bits {
                    if unit_size > 0 {
                        offset += unit_size;
                        bit_pos = 0;
                    }
                    let align = type_align(&m.ctype, struct_defs);
                    offset = align_to(offset, align);
                    unit_size = type_sz;
                }

                m.offset = offset;
                m.bit_offset = bit_pos;
                bit_pos += m.bit_width;
            } else {
                if unit_size > 0 {
                    offset += unit_size;
                    bit_pos = 0;
                    unit_size = 0;
                }

                let align = type_align(&m.ctype, struct_defs);
                offset = align_to(offset, align);
                m.offset = offset;
                m.bit_offset = 0;
                offset += type_size(&m.ctype, struct_defs);
            }
        }

        if unit_size > 0 {
            offset += unit_size;
        }

        let max_align = members.iter()
            .map(|m| type_align(&m.ctype, struct_defs))
            .max().unwrap_or(1);
        align_to(offset, max_align)
    }
}

fn number_type_str(nv: &crate::token::NumberVal) -> String {
    use crate::token::IntSize;
    if nv.is_real {
        match nv.size {
            IntSize::Int => "float".to_string(),
            IntSize::Long => "double".to_string(),
            IntSize::LongLong => "long double".to_string(),
        }
    } else {
        let base = match nv.size {
            IntSize::Int => "int",
            IntSize::Long => "long",
            IntSize::LongLong => "long long",
        };
        if nv.unsigned {
            format!("unsigned {}", base)
        } else {
            base.to_string()
        }
    }
}

fn number_val_str(nv: &crate::token::NumberVal) -> String {
    if nv.is_real {
        nv.real_str.clone()
    } else {
        nv.int_val.to_string()
    }
}
