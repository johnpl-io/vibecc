#[derive(Debug, Clone, PartialEq)]
pub enum Signedness {
    Default,
    Signed,
    Unsigned,
}

#[derive(Debug, Clone)]
pub struct CType {
    pub kind: TypeKind,
    pub is_const: bool,
    pub is_volatile: bool,
}

impl CType {
    pub fn new(kind: TypeKind) -> Self {
        CType { kind, is_const: false, is_volatile: false }
    }
}

#[derive(Debug, Clone)]
pub enum TypeKind {
    Void,
    Char(Signedness),
    Short(Signedness),
    Int(Signedness),
    Long(Signedness),
    LongLong(Signedness),
    Float,
    Double,
    LongDouble,
    Pointer(Box<CType>),
    Array { base: Box<CType>, size: Option<u64> },
    Function { return_type: Box<CType>, params: FuncParams },
    StructRef { is_union: bool, tag: Option<String>, def_id: Option<usize> },
}

#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub ctype: CType,
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub enum FuncParams {
    Unknown,
    Void,
    Params(Vec<ParamInfo>),
    KnR(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub is_union: bool,
    pub tag: Option<String>,
    pub members: Vec<StructMember>,
    pub size: u32,
    pub def_file: String,
    pub def_line: u32,
}

#[derive(Debug, Clone)]
pub struct StructMember {
    pub name: Option<String>,
    pub ctype: CType,
    pub offset: u32,
    pub bit_offset: u32,
    pub bit_width: u32,
    pub file: String,
    pub line: u32,
}

// Type printing

pub fn print_type(ctype: &CType, indent: usize, struct_defs: &[StructDef]) {
    let pad = " ".repeat(indent);
    let quals = qual_prefix(ctype);
    match &ctype.kind {
        TypeKind::Void | TypeKind::Char(_) | TypeKind::Short(_) | TypeKind::Int(_) |
        TypeKind::Long(_) | TypeKind::LongLong(_) | TypeKind::Float |
        TypeKind::Double | TypeKind::LongDouble => {
            println!("{}{}{}", pad, quals, scalar_name(&ctype.kind));
        }
        TypeKind::Pointer(inner) => {
            println!("{}{}pointer to ", pad, quals);
            print_type(inner, indent + 1, struct_defs);
        }
        TypeKind::Array { base, size } => {
            if let Some(n) = size {
                println!("{}{}array of  {} elements of type", pad, quals, n);
            } else {
                println!("{}{}array of unknown size, type", pad, quals);
            }
            print_type(base, indent + 1, struct_defs);
        }
        TypeKind::Function { return_type, params } => {
            println!("{}{}function returning", pad, quals);
            print_type(return_type, indent + 1, struct_defs);
            print_func_params(params, indent, struct_defs);
        }
        TypeKind::StructRef { is_union, tag, def_id } => {
            let kind = if *is_union { "union" } else { "struct" };
            let tag_str = tag.as_deref().unwrap_or("(anonymous)");
            if let Some(id) = def_id {
                let def = &struct_defs[*id];
                println!("{}{}{} {} (defined at {}:{})", pad, quals, kind, tag_str,
                    def.def_file, def.def_line);
            } else {
                println!("{}{}{} {} (incomplete)", pad, quals, kind, tag_str);
            }
        }
    }
}

pub fn print_func_params(params: &FuncParams, indent: usize, struct_defs: &[StructDef]) {
    let pad = " ".repeat(indent);
    match params {
        FuncParams::Void => println!("{}and taking no arguments", pad),
        FuncParams::Unknown => println!("{}and taking unknown arguments", pad),
        FuncParams::Params(params) => {
            println!("{}and taking the following arguments", pad);
            for p in params {
                print_type(&p.ctype, indent + 1, struct_defs);
            }
        }
        FuncParams::KnR(names) => {
            println!("{}and taking unknown arguments [named {}]", pad, names.join(" "));
        }
    }
}

fn qual_prefix(ctype: &CType) -> String {
    let mut parts = Vec::new();
    if ctype.is_const { parts.push("const"); }
    if ctype.is_volatile { parts.push("volatile"); }
    if parts.is_empty() {
        String::new()
    } else {
        format!("{} ", parts.join(" "))
    }
}

fn scalar_name(kind: &TypeKind) -> String {
    match kind {
        TypeKind::Void => "void".to_string(),
        TypeKind::Char(s) => format!("{}char", sign_str(s)),
        TypeKind::Short(s) => format!("{}short", sign_str(s)),
        TypeKind::Int(s) => format!("{}int", sign_str(s)),
        TypeKind::Long(s) => format!("{}long", sign_str(s)),
        TypeKind::LongLong(s) => format!("{}long long", sign_str(s)),
        TypeKind::Float => "float".to_string(),
        TypeKind::Double => "double".to_string(),
        TypeKind::LongDouble => "long double".to_string(),
        _ => String::new(),
    }
}

fn sign_str(s: &Signedness) -> &'static str {
    match s {
        Signedness::Default => "",
        Signedness::Signed => "signed ",
        Signedness::Unsigned => "unsigned ",
    }
}

// Size and alignment (32-bit target)

pub fn type_size(ctype: &CType, struct_defs: &[StructDef]) -> u32 {
    match &ctype.kind {
        TypeKind::Void => 0,
        TypeKind::Char(_) => 1,
        TypeKind::Short(_) => 2,
        TypeKind::Int(_) | TypeKind::Long(_) | TypeKind::Float => 4,
        TypeKind::LongLong(_) | TypeKind::Double => 8,
        TypeKind::LongDouble => 12,
        TypeKind::Pointer(_) => 4,
        TypeKind::Array { base, size } => {
            size.unwrap_or(0) as u32 * type_size(base, struct_defs)
        }
        TypeKind::Function { .. } => 0,
        TypeKind::StructRef { def_id, .. } => {
            def_id.map(|id| struct_defs[id].size).unwrap_or(0)
        }
    }
}

pub fn type_align(ctype: &CType, struct_defs: &[StructDef]) -> u32 {
    match &ctype.kind {
        TypeKind::Void | TypeKind::Char(_) => 1,
        TypeKind::Short(_) => 2,
        TypeKind::Int(_) | TypeKind::Long(_) | TypeKind::Float | TypeKind::Pointer(_) => 4,
        TypeKind::LongLong(_) | TypeKind::Double => 4, // 32-bit x86: 4-byte aligned
        TypeKind::LongDouble => 4,
        TypeKind::Array { base, .. } => type_align(base, struct_defs),
        TypeKind::Function { .. } => 1,
        TypeKind::StructRef { def_id, .. } => {
            if let Some(id) = def_id {
                struct_defs[*id].members.iter()
                    .map(|m| type_align(&m.ctype, struct_defs))
                    .max().unwrap_or(1)
            } else {
                1
            }
        }
    }
}

pub fn align_to(offset: u32, align: u32) -> u32 {
    if align == 0 { return offset; }
    (offset + align - 1) / align * align
}
