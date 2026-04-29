use crate::types::*;
use crate::symtab::*;

pub fn scope_kind_str(kind: &ScopeKind) -> &'static str {
    match kind {
        ScopeKind::Global => "global",
        ScopeKind::Function => "function",
        ScopeKind::Block => "block",
        ScopeKind::StructUnion => "struct/union",
    }
}

pub fn stgclass_str(stg: &StorageClass) -> &'static str {
    match stg {
        StorageClass::Auto => "auto",
        StorageClass::Static => "static",
        StorageClass::Extern => "extern",
        StorageClass::Register => "register",
    }
}

fn print_decl_header(name: &str, file: &str, line: u32, scope: &Scope) {
    print!("{} is defined at {}:{} [in {} scope starting at {}:{}] as a \n",
        name, file, line,
        scope_kind_str(&scope.kind), scope.start_file, scope.start_line);
}

pub fn print_variable(name: &str, file: &str, line: u32, ctype: &CType,
                      stg: &StorageClass, arg_num: Option<u32>,
                      scope: &Scope, struct_defs: &[StructDef]) {
    print_decl_header(name, file, line, scope);
    if let Some(n) = arg_num {
        println!("variable (argument #{}) of type:", n);
    } else {
        println!("variable with stgclass {}  of type:", stgclass_str(stg));
    }
    print_type(ctype, 2, struct_defs);
}

pub fn print_function(name: &str, file: &str, line: u32, ctype: &CType,
                      stg: &StorageClass, scope: &Scope, struct_defs: &[StructDef]) {
    print_decl_header(name, file, line, scope);
    if let TypeKind::Function { return_type, params } = &ctype.kind {
        println!("{}   function returning", stgclass_str(stg));
        print_type(return_type, 3, struct_defs);
        print_func_params(params, 2, struct_defs);
    }
}

pub fn print_typedef(name: &str, file: &str, line: u32, ctype: &CType,
                     scope: &Scope, struct_defs: &[StructDef]) {
    print_decl_header(name, file, line, scope);
    println!("typedef equivalent to:");
    print_type(ctype, 2, struct_defs);
}

pub fn print_field(member: &StructMember, scope: &Scope,
                   struct_tag: &str, is_union: bool, struct_defs: &[StructDef]) {
    let name = member.name.as_deref().unwrap_or("(anonymous)");
    let kind = if is_union { "union" } else { "struct" };
    print_decl_header(name, &member.file, member.line, scope);
    println!("field of {} {}  off={} bit_off={} bit_wid={}, type:",
        kind, struct_tag, member.offset, member.bit_offset, member.bit_width);
    print_type(&member.ctype, 2, struct_defs);
}

pub fn print_struct_def(def: &StructDef, scope: &Scope, struct_defs: &[StructDef]) {
    let kind = if def.is_union { "union" } else { "struct" };
    let tag = def.tag.as_deref().unwrap_or("(anonymous)");
    println!("{} {} definition at {}:{}{}", kind, tag, def.def_file, def.def_line, '{');

    // Sort members alphabetically for printing
    let mut sorted: Vec<&StructMember> = def.members.iter()
        .filter(|m| m.name.is_some())
        .collect();
    sorted.sort_by_key(|m| m.name.as_ref().unwrap().clone());

    for member in sorted {
        print_field(member, scope, tag, def.is_union, struct_defs);
    }

    println!("}} (size=={})", def.size);
    println!();
}

pub fn print_symbol_info(sym: &Symbol, scope: &Scope, struct_defs: &[StructDef]) {
    if sym.is_typedef {
        print_typedef(&sym.name, &sym.file, sym.line, &sym.ctype, scope, struct_defs);
    } else if matches!(&sym.ctype.kind, TypeKind::Function { .. }) {
        print_function(&sym.name, &sym.file, sym.line, &sym.ctype,
                      &sym.storage_class, scope, struct_defs);
    } else {
        print_variable(&sym.name, &sym.file, sym.line, &sym.ctype,
                      &sym.storage_class, sym.arg_num, scope, struct_defs);
    }
}
