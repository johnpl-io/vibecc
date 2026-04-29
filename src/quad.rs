use std::fmt;

#[derive(Debug, Clone)]
pub enum SymClass {
    Global,
    Lvar,
    Param(u32),
}

#[derive(Debug, Clone)]
pub enum Operand {
    Const(i64),
    Sym { name: String, class: SymClass },
    Temp(u32),
    FnAddr(String),
    StringLit(u32),
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operand::Const(n) => write!(f, "{}", n),
            Operand::Sym { name, class } => match class {
                SymClass::Global => write!(f, "{}{{global}}", name),
                SymClass::Lvar => write!(f, "{}{{lvar}}", name),
                SymClass::Param(_) => write!(f, "{}{{param}}", name),
            },
            Operand::Temp(n) => write!(f, "%T{:05}", n),
            Operand::FnAddr(name) => write!(f, "${}", name),
            Operand::StringLit(idx) => write!(f, "$.LC{}", idx),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CC {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CC {
    pub fn invert(self) -> CC {
        match self {
            CC::Eq => CC::Ne,
            CC::Ne => CC::Eq,
            CC::Lt => CC::Ge,
            CC::Le => CC::Gt,
            CC::Gt => CC::Le,
            CC::Ge => CC::Lt,
        }
    }

    pub fn mnemonic(self) -> &'static str {
        match self {
            CC::Eq => "BREQ",
            CC::Ne => "BRNE",
            CC::Lt => "BRLT",
            CC::Le => "BRLE",
            CC::Gt => "BRGT",
            CC::Ge => "BRGE",
        }
    }
}

/// A label for a basic block: .BB<F>.<N>
#[derive(Debug, Clone, Copy)]
pub struct BBLabel {
    pub fn_idx: u32,
    pub bb_id: u32,
}

impl fmt::Display for BBLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, ".BB{}.{}", self.fn_idx, self.bb_id)
    }
}

#[derive(Debug, Clone)]
pub enum Quad {
    Mov(Operand, Operand),                       // dest = MOV src
    Add(Operand, Operand, Operand),              // dest = ADD a,b
    Sub(Operand, Operand, Operand),
    Mul(Operand, Operand, Operand),
    Div(Operand, Operand, Operand),
    Mod(Operand, Operand, Operand),
    Lea(Operand, Operand),                       // dest = LEA src   (src is variable)
    Load(Operand, Operand),                      // dest = LOAD [ptr]
    Store(Operand, Operand),                     // STORE val,[ptr]
    Cmp(Operand, Operand),                       // CMP a,b
    Br(BBLabel),                                 // BR .BB
    BrCC(CC, BBLabel, BBLabel),                  // BRcc true_bb,false_bb
    Arg(u32, Operand),                           // ARG pos,val
    Call(Option<Operand>, Operand, u32),         // [dest =] CALL fn,nargs
    Return(Option<Operand>),
}

pub struct BasicBlock {
    pub label: BBLabel,
    pub quads: Vec<Quad>,
}

pub struct LocalSymInfo {
    pub name: String,
    pub size: u32,
}

pub struct FunctionQuads {
    pub name: String,
    pub blocks: Vec<BasicBlock>,
    pub locals: Vec<LocalSymInfo>,
    pub max_temp: u32,
}

pub fn print_function_quads(fq: &FunctionQuads) {
    println!("{}:", fq.name);
    for bb in &fq.blocks {
        println!("{}", bb.label);
        for q in &bb.quads {
            print_quad(q);
        }
    }
}

fn print_quad(q: &Quad) {
    match q {
        Quad::Mov(d, s) => println!("\t{} = MOV {}", d, s),
        Quad::Add(d, a, b) => println!("\t{} = ADD {},{}", d, a, b),
        Quad::Sub(d, a, b) => println!("\t{} = SUB {},{}", d, a, b),
        Quad::Mul(d, a, b) => println!("\t{} = MUL {},{}", d, a, b),
        Quad::Div(d, a, b) => println!("\t{} = DIV {},{}", d, a, b),
        Quad::Mod(d, a, b) => println!("\t{} = MOD {},{}", d, a, b),
        Quad::Lea(d, s) => println!("\t{} = LEA {}", d, s),
        Quad::Load(d, p) => println!("\t{} = LOAD [{}]", d, p),
        Quad::Store(v, p) => println!("\tSTORE {},[{}]", v, p),
        Quad::Cmp(a, b) => println!("\tCMP {},{}", a, b),
        Quad::Br(t) => println!("\tBR {}", t),
        Quad::BrCC(cc, t, f) => println!("\t{} {},{}", cc.mnemonic(), t, f),
        Quad::Arg(n, v) => println!("\tARG {},{}", n, v),
        Quad::Call(Some(d), f, n) => println!("\t{} = CALL {},{}", d, f, n),
        Quad::Call(None, f, n) => println!("\tCALL {},{}", f, n),
        Quad::Return(Some(v)) => println!("\tRETURN {}", v),
        Quad::Return(None) => println!("\tRETURN"),
    }
}
