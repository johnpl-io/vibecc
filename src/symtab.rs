use crate::types::CType;

#[derive(Debug, Clone, PartialEq)]
pub enum ScopeKind {
    Global,
    Function,
    Block,
    StructUnion,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StorageClass {
    Auto,
    Static,
    Extern,
    Register,
}

pub struct Symbol {
    pub name: String,
    pub ctype: CType,
    pub storage_class: StorageClass,
    pub is_typedef: bool,
    pub file: String,
    pub line: u32,
    pub arg_num: Option<u32>,
}

pub struct TagEntry {
    pub tag: String,
    pub is_union: bool,
    pub def_id: Option<usize>,
}

pub struct Scope {
    pub kind: ScopeKind,
    pub start_file: String,
    pub start_line: u32,
    pub ordinary: Vec<Symbol>,
    pub tags: Vec<TagEntry>,
}

pub struct SymbolTable {
    pub scopes: Vec<Scope>,
}

impl SymbolTable {
    pub fn new() -> Self {
        SymbolTable {
            scopes: vec![Scope {
                kind: ScopeKind::Global,
                start_file: String::new(),
                start_line: 1,
                ordinary: Vec::new(),
                tags: Vec::new(),
            }],
        }
    }

    pub fn push_scope(&mut self, kind: ScopeKind, file: String, line: u32) {
        self.scopes.push(Scope {
            kind,
            start_file: file,
            start_line: line,
            ordinary: Vec::new(),
            tags: Vec::new(),
        });
    }

    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub fn current_scope(&self) -> &Scope {
        self.scopes.last().unwrap()
    }

    pub fn current_scope_mut(&mut self) -> &mut Scope {
        self.scopes.last_mut().unwrap()
    }

    pub fn insert_symbol(&mut self, sym: Symbol) {
        self.current_scope_mut().ordinary.push(sym);
    }

    /// Insert tag into the nearest non-struct/union scope
    pub fn insert_tag(&mut self, entry: TagEntry) {
        for scope in self.scopes.iter_mut().rev() {
            if scope.kind != ScopeKind::StructUnion {
                scope.tags.push(entry);
                return;
            }
        }
        self.scopes[0].tags.push(entry);
    }

    /// Look up tag walking the scope chain (skipping struct scopes for tags)
    pub fn lookup_tag(&self, name: &str) -> Option<&TagEntry> {
        for scope in self.scopes.iter().rev() {
            for tag in scope.tags.iter().rev() {
                if tag.tag == name {
                    return Some(tag);
                }
            }
        }
        None
    }

    /// Look up tag only in the nearest non-struct scope
    pub fn lookup_tag_current(&self, name: &str) -> Option<&TagEntry> {
        for scope in self.scopes.iter().rev() {
            if scope.kind != ScopeKind::StructUnion {
                for tag in scope.tags.iter().rev() {
                    if tag.tag == name {
                        return Some(tag);
                    }
                }
                return None;
            }
        }
        None
    }

    /// Update a tag's def_id in the nearest non-struct scope
    pub fn update_tag_def(&mut self, name: &str, new_def_id: usize) {
        for scope in self.scopes.iter_mut().rev() {
            if scope.kind != ScopeKind::StructUnion {
                for tag in scope.tags.iter_mut().rev() {
                    if tag.tag == name {
                        tag.def_id = Some(new_def_id);
                        return;
                    }
                }
                return;
            }
        }
    }

    pub fn lookup_symbol(&self, name: &str) -> Option<&Symbol> {
        for scope in self.scopes.iter().rev() {
            for sym in scope.ordinary.iter().rev() {
                if sym.name == name {
                    return Some(sym);
                }
            }
        }
        None
    }

    pub fn lookup_with_scope(&self, name: &str) -> Option<(usize, &Symbol)> {
        for (i, scope) in self.scopes.iter().enumerate().rev() {
            for sym in scope.ordinary.iter().rev() {
                if sym.name == name {
                    return Some((i, sym));
                }
            }
        }
        None
    }

    pub fn lookup_symbol_current_scope(&self, name: &str) -> Option<&Symbol> {
        let scope = self.current_scope();
        for sym in scope.ordinary.iter().rev() {
            if sym.name == name {
                return Some(sym);
            }
        }
        None
    }

    pub fn is_typedef_name(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            for sym in scope.ordinary.iter().rev() {
                if sym.name == name {
                    return sym.is_typedef;
                }
            }
        }
        false
    }

    pub fn is_global_scope(&self) -> bool {
        self.scopes.last().map(|s| s.kind == ScopeKind::Global).unwrap_or(false)
    }

    pub fn insert_symbol_global(&mut self, sym: Symbol) {
        self.scopes[0].ordinary.push(sym);
    }

    pub fn set_arg_num(&mut self, name: &str, arg_num: u32) {
        let scope = self.scopes.last_mut().unwrap();
        for sym in scope.ordinary.iter_mut().rev() {
            if sym.name == name {
                sym.arg_num = Some(arg_num);
                return;
            }
        }
    }
}
