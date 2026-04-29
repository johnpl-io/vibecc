use std::collections::HashMap;
use crate::quad::*;
use crate::parser::GlobalDecl;

/// Per-function stack frame layout
struct Layout {
    /// Map sym name → ebp-relative offset (negative for locals, positive for params)
    locals: HashMap<String, i32>,
    /// Map temp index → ebp-relative offset (negative)
    temps: HashMap<u32, i32>,
    /// Total bytes to subtract from %esp in prologue
    frame_size: u32,
}

pub fn emit_translation_unit(
    out: &mut String,
    globals: &[GlobalDecl],
    strings: &[String],
    functions: &[FunctionQuads],
    file_name: &str,
) {
    out.push_str(&format!("\t.file \"{}\"\n", file_name));

    // Functions in .text
    if !functions.is_empty() {
        out.push_str("\t.text\n");
        for f in functions {
            emit_function(out, f);
        }
    }

    // String literals in .rodata
    if !strings.is_empty() {
        out.push_str("\t.section\t.rodata\n");
        for (i, s) in strings.iter().enumerate() {
            out.push_str(&format!(".LC{}:\n", i));
            out.push_str(&format!("\t.string \"{}\"\n", escape_for_string(s)));
        }
    }

    // Uninitialized globals via .comm
    for g in globals {
        if g.is_function { continue; }
        out.push_str(&format!("\t.comm {},{},{}\n", g.name, g.size, g.align));
    }
}

fn escape_for_string(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            c if (c as u32) < 0x20 || (c as u32) >= 0x7f => {
                out.push_str(&format!("\\{:03o}", c as u32 & 0xff));
            }
            _ => out.push(c),
        }
    }
    out
}

fn build_layout(f: &FunctionQuads) -> Layout {
    let mut locals = HashMap::new();
    let mut temps = HashMap::new();
    let mut offset: i32 = 0;

    // Locals (non-param locals get negative offsets)
    for l in &f.locals {
        let sz = align_up(l.size, 4) as i32;
        offset -= sz;
        locals.insert(l.name.clone(), offset);
    }

    // Temps each get 4-byte slot
    for t in 1..=f.max_temp {
        offset -= 4;
        temps.insert(t, offset);
    }

    let frame_size = align_up((-offset) as u32, 16);
    Layout { locals, temps, frame_size }
}

fn align_up(n: u32, align: u32) -> u32 {
    if align == 0 { return n; }
    (n + align - 1) / align * align
}

fn emit_function(out: &mut String, f: &FunctionQuads) {
    let layout = build_layout(f);
    out.push_str(&format!("\t.globl {}\n", f.name));
    out.push_str(&format!("\t.type {}, @function\n", f.name));
    out.push_str(&format!("{}:\n", f.name));
    out.push_str("\tpushl %ebp\n");
    out.push_str("\tmovl %esp, %ebp\n");
    if layout.frame_size > 0 {
        out.push_str(&format!("\tsubl ${}, %esp\n", layout.frame_size));
    }

    for bb in &f.blocks {
        out.push_str(&format!(".BB{}_{}:\n", bb.label.fn_idx, bb.label.bb_id));
        for q in &bb.quads {
            emit_quad(out, q, &layout, f);
        }
    }
}

fn bb_label(l: &BBLabel) -> String {
    format!(".BB{}_{}", l.fn_idx, l.bb_id)
}

fn load_to(out: &mut String, op: &Operand, reg: &str, layout: &Layout, f: &FunctionQuads) {
    let s = op_value(op, layout, f);
    out.push_str(&format!("\tmovl {}, {}\n", s, reg));
}

fn store_from(out: &mut String, reg: &str, op: &Operand, layout: &Layout, f: &FunctionQuads) {
    let d = op_dest(op, layout, f);
    out.push_str(&format!("\tmovl {}, {}\n", reg, d));
}

fn op_value(op: &Operand, layout: &Layout, _f: &FunctionQuads) -> String {
    match op {
        Operand::Const(n) => format!("${}", n),
        Operand::Sym { name, class } => match class {
            SymClass::Global => name.clone(),
            SymClass::Lvar => format!("{}(%ebp)", layout.locals.get(name).copied().unwrap_or(0)),
            SymClass::Param(n) => format!("{}(%ebp)", 4 + 4 * (*n as i32)),
        },
        Operand::Temp(t) => format!("{}(%ebp)", layout.temps.get(t).copied().unwrap_or(0)),
        Operand::FnAddr(name) => format!("${}", name),
        Operand::StringLit(idx) => format!("$.LC{}", idx),
    }
}

/// Operand as a destination memory operand (cannot be an immediate).
fn op_dest(op: &Operand, layout: &Layout, _f: &FunctionQuads) -> String {
    match op {
        Operand::Const(_) => panic!("Const cannot be destination"),
        Operand::Sym { name, class } => match class {
            SymClass::Global => name.clone(),
            SymClass::Lvar => format!("{}(%ebp)", layout.locals.get(name).copied().unwrap_or(0)),
            SymClass::Param(n) => format!("{}(%ebp)", 4 + 4 * (*n as i32)),
        },
        Operand::Temp(t) => format!("{}(%ebp)", layout.temps.get(t).copied().unwrap_or(0)),
        Operand::FnAddr(_) => panic!("FnAddr cannot be destination"),
        Operand::StringLit(_) => panic!("StringLit cannot be destination"),
    }
}

fn emit_quad(out: &mut String, q: &Quad, layout: &Layout, f: &FunctionQuads) {
    match q {
        Quad::Mov(d, s) => {
            load_to(out, s, "%eax", layout, f);
            store_from(out, "%eax", d, layout, f);
        }
        Quad::Add(d, a, b) => {
            load_to(out, a, "%eax", layout, f);
            out.push_str(&format!("\taddl {}, %eax\n", op_value(b, layout, f)));
            store_from(out, "%eax", d, layout, f);
        }
        Quad::Sub(d, a, b) => {
            load_to(out, a, "%eax", layout, f);
            out.push_str(&format!("\tsubl {}, %eax\n", op_value(b, layout, f)));
            store_from(out, "%eax", d, layout, f);
        }
        Quad::Mul(d, a, b) => {
            load_to(out, a, "%eax", layout, f);
            out.push_str(&format!("\timull {}, %eax\n", op_value(b, layout, f)));
            store_from(out, "%eax", d, layout, f);
        }
        Quad::Div(d, a, b) => {
            load_to(out, a, "%eax", layout, f);
            out.push_str("\tcltd\n");
            // idivl needs reg/mem divisor (not immediate)
            match b {
                Operand::Const(_) => {
                    out.push_str(&format!("\tmovl {}, %ecx\n", op_value(b, layout, f)));
                    out.push_str("\tidivl %ecx\n");
                }
                _ => {
                    out.push_str(&format!("\tidivl {}\n", op_value(b, layout, f)));
                }
            }
            store_from(out, "%eax", d, layout, f);
        }
        Quad::Mod(d, a, b) => {
            load_to(out, a, "%eax", layout, f);
            out.push_str("\tcltd\n");
            match b {
                Operand::Const(_) => {
                    out.push_str(&format!("\tmovl {}, %ecx\n", op_value(b, layout, f)));
                    out.push_str("\tidivl %ecx\n");
                }
                _ => {
                    out.push_str(&format!("\tidivl {}\n", op_value(b, layout, f)));
                }
            }
            store_from(out, "%edx", d, layout, f);
        }
        Quad::Lea(d, s) => {
            match s {
                Operand::Sym { name, class } => match class {
                    SymClass::Lvar => {
                        let off = layout.locals.get(name).copied().unwrap_or(0);
                        out.push_str(&format!("\tleal {}(%ebp), %eax\n", off));
                    }
                    SymClass::Param(n) => {
                        let off = 4 + 4 * (*n as i32);
                        out.push_str(&format!("\tleal {}(%ebp), %eax\n", off));
                    }
                    SymClass::Global => {
                        out.push_str(&format!("\tmovl ${}, %eax\n", name));
                    }
                },
                Operand::FnAddr(name) => {
                    out.push_str(&format!("\tmovl ${}, %eax\n", name));
                }
                Operand::StringLit(idx) => {
                    out.push_str(&format!("\tmovl $.LC{}, %eax\n", idx));
                }
                _ => {
                    // Fallback: treat as value
                    load_to(out, s, "%eax", layout, f);
                }
            }
            store_from(out, "%eax", d, layout, f);
        }
        Quad::Load(d, p) => {
            load_to(out, p, "%eax", layout, f);
            out.push_str("\tmovl (%eax), %edx\n");
            store_from(out, "%edx", d, layout, f);
        }
        Quad::Store(v, p) => {
            load_to(out, v, "%eax", layout, f);
            load_to(out, p, "%edx", layout, f);
            out.push_str("\tmovl %eax, (%edx)\n");
        }
        Quad::Cmp(a, b) => {
            load_to(out, a, "%eax", layout, f);
            out.push_str(&format!("\tcmpl {}, %eax\n", op_value(b, layout, f)));
        }
        Quad::Br(t) => {
            out.push_str(&format!("\tjmp {}\n", bb_label(t)));
        }
        Quad::BrCC(cc, t, fls) => {
            let mn = match cc {
                CC::Eq => "je",
                CC::Ne => "jne",
                CC::Lt => "jl",
                CC::Le => "jle",
                CC::Gt => "jg",
                CC::Ge => "jge",
            };
            out.push_str(&format!("\t{} {}\n", mn, bb_label(t)));
            out.push_str(&format!("\tjmp {}\n", bb_label(fls)));
        }
        Quad::Arg(_, val) => {
            out.push_str(&format!("\tpushl {}\n", op_value(val, layout, f)));
        }
        Quad::Call(dest, fn_op, n) => {
            match fn_op {
                Operand::FnAddr(name) => {
                    out.push_str(&format!("\tcall {}\n", name));
                }
                _ => {
                    load_to(out, fn_op, "%eax", layout, f);
                    out.push_str("\tcall *%eax\n");
                }
            }
            if *n > 0 {
                out.push_str(&format!("\taddl ${}, %esp\n", 4 * n));
            }
            if let Some(d) = dest {
                store_from(out, "%eax", d, layout, f);
            }
        }
        Quad::Return(opt) => {
            if let Some(v) = opt {
                load_to(out, v, "%eax", layout, f);
            }
            out.push_str("\tleave\n");
            out.push_str("\tret\n");
        }
    }
}

