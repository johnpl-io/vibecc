/// Token kinds matching tokens-manual.h
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Single-char tokens represented by ASCII value - stored as char
    Char(char),

    // Multi-char / variable-length tokens
    Ident(String),
    Number(NumberVal),
    CharLit(Vec<u8>),
    Str(Vec<u8>),

    // Multi-char operators
    Indsel,      // ->
    Plusplus,    // ++
    Minusminus,  // --
    Shl,         // <<
    Shr,         // >>
    Lteq,        // <=
    Gteq,        // >=
    Eqeq,        // ==
    Noteq,       // !=
    Logand,      // &&
    Logor,       // ||
    Ellipsis,    // ...
    Timeseq,     // *=
    Diveq,       // /=
    Modeq,       // %=
    Pluseq,      // +=
    Minuseq,     // -=
    Shleq,       // <<=
    Shreq,       // >>=
    Andeq,       // &=
    Oreq,        // |=
    Xoreq,       // ^=

    // Keywords
    Auto, Break, Case, KwChar, Const, Continue, Default, Do, Double,
    Else, Enum, Extern, Float, For, Goto, If, Inline, Int, Long,
    Register, Restrict, Return, Short, Signed, Sizeof, Static, Struct,
    Switch, Typedef, Union, Unsigned, Void, Volatile, While,
    Bool, Complex, Imaginary,

    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NumberVal {
    pub is_real: bool,
    pub int_val: u64,
    pub real_val: f64,
    pub real_str: String, // canonical string for real output
    pub unsigned: bool,
    pub size: IntSize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IntSize {
    Int,
    Long,
    LongLong,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub filename: String,
    pub line: u32,
}
