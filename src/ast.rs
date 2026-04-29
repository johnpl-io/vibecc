use crate::quad::SymClass;
use crate::types::CType;

#[derive(Debug, Clone)]
pub enum AstNode {
    // Expressions
    StabVar { name: String, file: String, line: u32, class: SymClass, ctype: Option<CType> },
    StabFn { name: String, file: String, line: u32, ctype: Option<CType> },
    Number { numtype: String, val: String },
    Str(String),
    BinaryOp { op: String, left: Box<AstNode>, right: Box<AstNode> },
    LogicalOp { op: String, left: Box<AstNode>, right: Box<AstNode> },
    ComparisonOp { op: String, left: Box<AstNode>, right: Box<AstNode> },
    UnaryOp { op: String, operand: Box<AstNode> },
    Assignment { left: Box<AstNode>, right: Box<AstNode> },
    Ternary { cond: Box<AstNode>, then_expr: Box<AstNode>, else_expr: Box<AstNode> },
    FnCall { func: Box<AstNode>, args: Vec<AstNode> },
    Deref(Box<AstNode>),
    AddressOf(Box<AstNode>),
    DirectSelect { obj: Box<AstNode>, member: String },
    IndirectSelect { obj: Box<AstNode>, member: String },
    Sizeof(Box<AstNode>),
    Comma { left: Box<AstNode>, right: Box<AstNode> },

    // Statements
    List(Vec<AstNode>),
    If { cond: Box<AstNode>, then_body: Box<AstNode>, else_body: Option<Box<AstNode>> },
    While { cond: Box<AstNode>, body: Box<AstNode> },
    DoWhile { body: Box<AstNode>, cond: Box<AstNode> },
    For {
        init: Option<Box<AstNode>>,
        cond: Option<Box<AstNode>>,
        incr: Option<Box<AstNode>>,
        body: Box<AstNode>,
    },
    Switch { expr: Box<AstNode>, body: Box<AstNode> },
    Case { expr: Box<AstNode>, stmt: Box<AstNode> },
    Default(Box<AstNode>),
    Break,
    Continue,
    Return(Option<Box<AstNode>>),
    Goto(String),
    Label { name: String, stmt: Box<AstNode> },
    Noop,
}

pub fn print_ast(node: &AstNode, indent: usize) {
    let pad = " ".repeat(indent);
    match node {
        AstNode::StabVar { name, file, line, .. } => {
            println!("{}stab_var name={} def @{}:{}", pad, name, file, line);
        }
        AstNode::StabFn { name, file, line, .. } => {
            println!("{}stab_fn name={} def @{}:{}", pad, name, file, line);
        }
        AstNode::Number { numtype, val } => {
            println!("{}CONSTANT: (type={}){}", pad, numtype, val);
        }
        AstNode::Str(s) => {
            let escaped = escape_string(s);
            println!("{}STRING\t{}", pad, escaped);
        }
        AstNode::BinaryOp { op, left, right } => {
            println!("{}BINARY OP {}", pad, op);
            print_ast(left, indent + 1);
            print_ast(right, indent + 1);
        }
        AstNode::LogicalOp { op, left, right } => {
            println!("{}LOGICAL OP {}", pad, op);
            print_ast(left, indent + 1);
            print_ast(right, indent + 1);
        }
        AstNode::ComparisonOp { op, left, right } => {
            println!("{}COMPARISON OP {}", pad, op);
            print_ast(left, indent + 1);
            print_ast(right, indent + 1);
        }
        AstNode::UnaryOp { op, operand } => {
            println!("{}UNARY OP {}", pad, op);
            print_ast(operand, indent + 1);
        }
        AstNode::Assignment { left, right } => {
            println!("{}ASSIGNMENT", pad);
            print_ast(left, indent + 1);
            print_ast(right, indent + 1);
        }
        AstNode::Ternary { cond, then_expr, else_expr } => {
            println!("{}TERNARY OP, IF:", pad);
            print_ast(cond, indent + 2);
            println!("{}THEN:", pad);
            print_ast(then_expr, indent + 1);
            println!("{}ELSE:", pad);
            print_ast(else_expr, indent + 1);
        }
        AstNode::FnCall { func, args } => {
            println!("{}FNCALL, {} arguments", pad, args.len());
            print_ast(func, indent + 1);
            for (i, arg) in args.iter().enumerate() {
                println!("{}arg #{}=", pad, i + 1);
                print_ast(arg, indent + 1);
            }
        }
        AstNode::Deref(child) => {
            println!("{}DEREF", pad);
            print_ast(child, indent + 1);
        }
        AstNode::AddressOf(child) => {
            println!("{}ADDRESSOF", pad);
            print_ast(child, indent + 1);
        }
        AstNode::DirectSelect { obj, member } => {
            println!("{}DIRECT SELECT, member {}", pad, member);
            print_ast(obj, indent + 1);
        }
        AstNode::IndirectSelect { obj, member } => {
            println!("{}INDIRECT SELECT, member {}", pad, member);
            print_ast(obj, indent + 1);
        }
        AstNode::Sizeof(child) => {
            println!("{}SIZEOF", pad);
            print_ast(child, indent + 1);
        }
        AstNode::Comma { left, right } => {
            println!("{}COMMA", pad);
            print_ast(left, indent + 1);
            print_ast(right, indent + 1);
        }

        // Statements
        AstNode::List(stmts) => {
            println!("{}LIST {{", pad);
            for s in stmts {
                print_ast(s, indent + 1);
            }
            println!("{}}}", pad);
        }
        AstNode::If { cond, then_body, else_body } => {
            println!("{}IF:", pad);
            print_ast(cond, indent + 1);
            println!("{}THEN:", pad);
            print_ast(then_body, indent + 1);
            if let Some(eb) = else_body {
                println!("{}ELSE:", pad);
                print_ast(eb, indent + 1);
            }
        }
        AstNode::While { cond, body } => {
            println!("{}WHILE", pad);
            println!("{}COND:", pad);
            print_ast(cond, indent + 1);
            println!("{}BODY:", pad);
            print_ast(body, indent + 1);
        }
        AstNode::DoWhile { body, cond } => {
            println!("{}DO-WHILE", pad);
            println!("{}BODY:", pad);
            print_ast(body, indent + 1);
            println!("{}COND:", pad);
            print_ast(cond, indent + 1);
        }
        AstNode::For { init, cond, incr, body } => {
            println!("{}FOR", pad);
            if let Some(i) = init {
                println!("{}INIT:", pad);
                print_ast(i, indent + 1);
            }
            if let Some(c) = cond {
                println!("{}COND:", pad);
                print_ast(c, indent + 1);
            }
            println!("{}BODY:", pad);
            print_ast(body, indent + 1);
            if let Some(inc) = incr {
                println!("{}INCR:", pad);
                print_ast(inc, indent + 1);
            }
        }
        AstNode::Switch { expr, body } => {
            println!("{}SWITCH, EXPR:", pad);
            print_ast(expr, indent + 1);
            println!("{}BODY:", pad);
            print_ast(body, indent + 1);
        }
        AstNode::Case { expr, stmt } => {
            println!("{}CASE", pad);
            println!("{} EXPR:", pad);
            print_ast(expr, indent + 2);
            println!("{} STMT:", pad);
            print_ast(stmt, indent + 2);
        }
        AstNode::Default(stmt) => {
            println!("{}DEFAULT", pad);
            print_ast(stmt, indent + 1);
        }
        AstNode::Break => {
            println!("{}BREAK", pad);
        }
        AstNode::Continue => {
            println!("{}CONTINUE", pad);
        }
        AstNode::Return(expr) => {
            println!("{}RETURN", pad);
            if let Some(e) = expr {
                print_ast(e, indent + 1);
            }
        }
        AstNode::Goto(label) => {
            println!("{}GOTO {} (DEF)", pad, label);
        }
        AstNode::Label { name, stmt } => {
            println!("{}LABEL({}):", pad, name);
            print_ast(stmt, indent + 1);
        }
        AstNode::Noop => {}
    }
}

fn escape_string(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\\' => out.push_str("\\\\"),
            '\0' => out.push_str("\\0"),
            c if c.is_ascii_control() => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            _ => out.push(c),
        }
    }
    out
}
