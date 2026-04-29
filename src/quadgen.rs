use crate::ast::AstNode;
use crate::quad::*;
use crate::types::{CType, TypeKind, StructDef, type_size};

/// Type information attached to expression results during quad gen.
#[derive(Clone)]
struct ExprResult {
    op: Operand,
    ty: Option<CType>,
}

pub struct QuadGen<'a> {
    fn_idx: u32,
    bb_counter: u32,
    temp_counter: u32,
    next_placeholder: u32,
    blocks: Vec<BasicBlock>,
    cursor: usize,
    break_targets: Vec<BBLabel>,
    continue_targets: Vec<BBLabel>,
    struct_defs: &'a [StructDef],
    strings: &'a mut Vec<String>,
}

impl<'a> QuadGen<'a> {
    pub fn new(fn_idx: u32, struct_defs: &'a [StructDef],
               strings: &'a mut Vec<String>) -> Self {
        let mut g = QuadGen {
            fn_idx,
            bb_counter: 0,
            temp_counter: 0,
            next_placeholder: 1_000_000,
            blocks: Vec::new(),
            cursor: 0,
            break_targets: Vec::new(),
            continue_targets: Vec::new(),
            struct_defs,
            strings,
        };
        g.new_bb();
        g
    }

    pub fn into_function(self, name: String, locals: Vec<LocalSymInfo>) -> FunctionQuads {
        FunctionQuads {
            name,
            blocks: self.blocks,
            locals,
            max_temp: self.temp_counter,
        }
    }

    fn new_bb(&mut self) -> BBLabel {
        self.bb_counter += 1;
        let label = BBLabel { fn_idx: self.fn_idx, bb_id: self.bb_counter };
        self.blocks.push(BasicBlock { label, quads: Vec::new() });
        self.cursor = self.blocks.len() - 1;
        label
    }

    fn new_placeholder(&mut self) -> BBLabel {
        self.next_placeholder += 1;
        BBLabel { fn_idx: self.fn_idx, bb_id: self.next_placeholder }
    }

    fn emit(&mut self, q: Quad) {
        self.blocks[self.cursor].quads.push(q);
    }

    fn new_temp(&mut self) -> Operand {
        self.temp_counter += 1;
        Operand::Temp(self.temp_counter)
    }

    fn patch_target(&mut self, placeholder_id: u32, to: BBLabel) {
        for bb in &mut self.blocks {
            for q in &mut bb.quads {
                match q {
                    Quad::Br(t) if t.bb_id == placeholder_id => *t = to,
                    Quad::BrCC(_, t, f) => {
                        if t.bb_id == placeholder_id { *t = to; }
                        if f.bb_id == placeholder_id { *f = to; }
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn gen_function_body(&mut self, body: &AstNode) {
        self.gen_stmt(body);
        // Drop trailing empty BBs that have no incoming branches
        loop {
            if self.blocks.len() < 2 { break; }
            let last_id = self.blocks.last().unwrap().label.bb_id;
            if !self.blocks.last().unwrap().quads.is_empty() { break; }
            if self.is_branch_target(last_id) { break; }
            self.blocks.pop();
        }
        self.cursor = self.blocks.len() - 1;
        if !current_terminated(&self.blocks[self.cursor]) {
            self.emit(Quad::Return(None));
        }
    }

    fn is_branch_target(&self, bb_id: u32) -> bool {
        for bb in &self.blocks {
            for q in &bb.quads {
                match q {
                    Quad::Br(t) if t.bb_id == bb_id => return true,
                    Quad::BrCC(_, t, f) if t.bb_id == bb_id || f.bb_id == bb_id => return true,
                    _ => {}
                }
            }
        }
        false
    }

    // ---------- Statement generation ----------

    fn gen_stmt(&mut self, stmt: &AstNode) {
        match stmt {
            AstNode::Noop => {}
            AstNode::List(items) => {
                for s in items {
                    self.gen_stmt(s);
                }
            }
            AstNode::If { cond, then_body, else_body } => {
                self.gen_if(cond, then_body, else_body.as_deref());
            }
            AstNode::While { cond, body } => {
                self.gen_while(cond, body);
            }
            AstNode::DoWhile { body, cond } => {
                self.gen_do_while(body, cond);
            }
            AstNode::For { init, cond, incr, body } => {
                self.gen_for(init.as_deref(), cond.as_deref(), incr.as_deref(), body);
            }
            AstNode::Break => {
                if let Some(t) = self.break_targets.last().copied() {
                    self.emit(Quad::Br(t));
                }
            }
            AstNode::Continue => {
                if let Some(t) = self.continue_targets.last().copied() {
                    self.emit(Quad::Br(t));
                }
            }
            AstNode::Return(opt) => {
                if let Some(e) = opt {
                    let r = self.gen_rvalue(e, None);
                    self.emit(Quad::Return(Some(r.op)));
                } else {
                    self.emit(Quad::Return(None));
                }
            }
            other => {
                let _ = self.gen_rvalue(other, None);
            }
        }
    }

    fn gen_if(&mut self, cond: &AstNode, then_body: &AstNode, else_body: Option<&AstNode>) {
        let join_ph = self.new_placeholder();
        let then_ph = self.new_placeholder();
        let else_ph = if else_body.is_some() { Some(self.new_placeholder()) } else { None };

        let false_target = else_ph.unwrap_or(join_ph);
        self.gen_cond_branch(cond, then_ph, false_target);

        // THEN block
        let then_bb = self.new_bb();
        self.patch_target(then_ph.bb_id, then_bb);
        self.gen_stmt(then_body);
        if !current_terminated(&self.blocks[self.cursor]) {
            self.emit(Quad::Br(join_ph));
        }

        // ELSE block
        if let (Some(eph), Some(eb)) = (else_ph, else_body) {
            let else_bb = self.new_bb();
            self.patch_target(eph.bb_id, else_bb);
            self.gen_stmt(eb);
            if !current_terminated(&self.blocks[self.cursor]) {
                self.emit(Quad::Br(join_ph));
            }
        }

        // JOIN block
        let join_bb = self.new_bb();
        self.patch_target(join_ph.bb_id, join_bb);
    }

    fn gen_while(&mut self, cond: &AstNode, body: &AstNode) {
        let cond_ph = self.new_placeholder();
        let body_ph = self.new_placeholder();
        let post_ph = self.new_placeholder();

        // Branch from current BB to cond
        self.emit(Quad::Br(cond_ph));

        // Cond block
        let cond_bb = self.new_bb();
        self.patch_target(cond_ph.bb_id, cond_bb);
        self.gen_cond_branch(cond, body_ph, post_ph);

        // Body block
        let body_bb = self.new_bb();
        self.patch_target(body_ph.bb_id, body_bb);
        self.break_targets.push(post_ph);
        self.continue_targets.push(cond_bb);
        self.gen_stmt(body);
        if !current_terminated(&self.blocks[self.cursor]) {
            self.emit(Quad::Br(cond_bb));
        }
        self.break_targets.pop();
        self.continue_targets.pop();

        // Post block
        let post_bb = self.new_bb();
        self.patch_target(post_ph.bb_id, post_bb);
    }

    fn gen_do_while(&mut self, body: &AstNode, cond: &AstNode) {
        let body_ph = self.new_placeholder();
        let cond_ph = self.new_placeholder();
        let post_ph = self.new_placeholder();

        self.emit(Quad::Br(body_ph));

        let body_bb = self.new_bb();
        self.patch_target(body_ph.bb_id, body_bb);
        self.break_targets.push(post_ph);
        self.continue_targets.push(cond_ph);
        self.gen_stmt(body);
        if !current_terminated(&self.blocks[self.cursor]) {
            self.emit(Quad::Br(cond_ph));
        }
        self.break_targets.pop();
        self.continue_targets.pop();

        let cond_bb = self.new_bb();
        self.patch_target(cond_ph.bb_id, cond_bb);
        self.gen_cond_branch(cond, body_bb, post_ph);

        let post_bb = self.new_bb();
        self.patch_target(post_ph.bb_id, post_bb);
    }

    fn gen_for(&mut self, init: Option<&AstNode>, cond: Option<&AstNode>,
               incr: Option<&AstNode>, body: &AstNode) {
        if let Some(i) = init {
            self.gen_stmt(i);
        }

        let cond_ph = self.new_placeholder();
        let body_ph = self.new_placeholder();
        let incr_ph = self.new_placeholder();
        let post_ph = self.new_placeholder();

        self.emit(Quad::Br(cond_ph));

        // Cond block
        let cond_bb = self.new_bb();
        self.patch_target(cond_ph.bb_id, cond_bb);
        if let Some(c) = cond {
            self.gen_cond_branch(c, body_ph, post_ph);
        } else {
            self.emit(Quad::Br(body_ph));
        }

        // Body block
        let body_bb = self.new_bb();
        self.patch_target(body_ph.bb_id, body_bb);
        self.break_targets.push(post_ph);
        self.continue_targets.push(incr_ph);
        self.gen_stmt(body);
        if !current_terminated(&self.blocks[self.cursor]) {
            self.emit(Quad::Br(incr_ph));
        }
        self.break_targets.pop();
        self.continue_targets.pop();

        // Incr block
        let incr_bb = self.new_bb();
        self.patch_target(incr_ph.bb_id, incr_bb);
        if let Some(i) = incr {
            let _ = self.gen_rvalue(i, None);
        }
        self.emit(Quad::Br(cond_bb));
        let _ = body_bb;

        // Post block
        let post_bb = self.new_bb();
        self.patch_target(post_ph.bb_id, post_bb);
    }

    fn gen_cond_branch(&mut self, cond: &AstNode, true_bb: BBLabel, false_bb: BBLabel) {
        match cond {
            AstNode::ComparisonOp { op, left, right } => {
                let l = self.gen_rvalue(left, None);
                let r = self.gen_rvalue(right, None);
                self.emit(Quad::Cmp(l.op, r.op));
                let cc = match op.as_str() {
                    "<" => CC::Lt,
                    "<=" => CC::Le,
                    ">" => CC::Gt,
                    ">=" => CC::Ge,
                    "==" => CC::Eq,
                    "!=" => CC::Ne,
                    _ => CC::Ne,
                };
                let inv = cc.invert();
                // BRcc<inv> false_bb,true_bb : if inv-cond true, take false branch
                self.emit(Quad::BrCC(inv, false_bb, true_bb));
            }
            _ => {
                let v = self.gen_rvalue(cond, None);
                self.emit(Quad::Cmp(v.op, Operand::Const(0)));
                self.emit(Quad::BrCC(CC::Eq, false_bb, true_bb));
            }
        }
    }

    // ---------- Expression generation ----------

    fn gen_rvalue(&mut self, expr: &AstNode, target: Option<Operand>) -> ExprResult {
        match expr {
            AstNode::Number { val, .. } => {
                let n: i64 = val.parse().unwrap_or(0);
                let v = Operand::Const(n);
                if let Some(t) = target {
                    self.emit(Quad::Mov(t.clone(), v));
                    ExprResult { op: t, ty: int_type() }
                } else {
                    ExprResult { op: v, ty: int_type() }
                }
            }
            AstNode::StabVar { name, class, ctype, .. } => {
                let class = class.clone();
                let ty = ctype.clone();
                if let Some(ref t) = ty {
                    if let TypeKind::Array { ref base, .. } = t.kind {
                        let dest = target.unwrap_or_else(|| self.new_temp());
                        let sym = Operand::Sym { name: name.clone(), class };
                        self.emit(Quad::Lea(dest.clone(), sym));
                        let pty = CType::new(TypeKind::Pointer(base.clone()));
                        return ExprResult { op: dest, ty: Some(pty) };
                    }
                }
                let sym_op = Operand::Sym { name: name.clone(), class };
                if let Some(t) = target {
                    self.emit(Quad::Mov(t.clone(), sym_op));
                    ExprResult { op: t, ty }
                } else {
                    ExprResult { op: sym_op, ty }
                }
            }
            AstNode::StabFn { name, ctype, .. } => {
                let dest = target.unwrap_or_else(|| self.new_temp());
                let f = Operand::FnAddr(name.clone());
                self.emit(Quad::Lea(dest.clone(), f));
                let ty = ctype.clone();
                ExprResult { op: dest, ty: ty.map(|t| CType::new(TypeKind::Pointer(Box::new(t)))) }
            }
            AstNode::BinaryOp { op, left, right } => {
                self.gen_arith(op, left, right, target)
            }
            AstNode::ComparisonOp { op, left, right } => {
                let dest = target.unwrap_or_else(|| self.new_temp());
                let true_ph = self.new_placeholder();
                let false_ph = self.new_placeholder();
                let join_ph = self.new_placeholder();
                let synth = AstNode::ComparisonOp {
                    op: op.clone(),
                    left: left.clone(),
                    right: right.clone(),
                };
                self.gen_cond_branch(&synth, true_ph, false_ph);

                let true_bb = self.new_bb();
                self.patch_target(true_ph.bb_id, true_bb);
                self.emit(Quad::Mov(dest.clone(), Operand::Const(1)));
                self.emit(Quad::Br(join_ph));

                let false_bb = self.new_bb();
                self.patch_target(false_ph.bb_id, false_bb);
                self.emit(Quad::Mov(dest.clone(), Operand::Const(0)));
                self.emit(Quad::Br(join_ph));

                let join_bb = self.new_bb();
                self.patch_target(join_ph.bb_id, join_bb);
                ExprResult { op: dest, ty: int_type() }
            }
            AstNode::UnaryOp { op, operand } => {
                self.gen_unary(op, operand, target)
            }
            AstNode::Assignment { left, right } => {
                self.gen_assign(left, right, target)
            }
            AstNode::Deref(inner) => {
                let addr = self.gen_rvalue(inner, None);
                let dest = target.unwrap_or_else(|| self.new_temp());
                self.emit(Quad::Load(dest.clone(), addr.op));
                let pointee_ty = match addr.ty {
                    Some(CType { kind: TypeKind::Pointer(b), .. }) => Some(*b),
                    Some(CType { kind: TypeKind::Array { base, .. }, .. }) => Some(*base),
                    _ => None,
                };
                ExprResult { op: dest, ty: pointee_ty }
            }
            AstNode::AddressOf(inner) => {
                self.gen_address(inner, target)
            }
            AstNode::FnCall { func, args } => {
                self.gen_call(func, args, target)
            }
            AstNode::Sizeof(inner) => {
                let r = self.gen_rvalue_typed_only(inner);
                let n = r.ty.as_ref().map(|t| type_size(t, self.struct_defs)).unwrap_or(0);
                let v = Operand::Const(n as i64);
                if let Some(t) = target {
                    self.emit(Quad::Mov(t.clone(), v));
                    ExprResult { op: t, ty: int_type() }
                } else {
                    ExprResult { op: v, ty: int_type() }
                }
            }
            AstNode::Comma { left, right } => {
                let _ = self.gen_rvalue(left, None);
                self.gen_rvalue(right, target)
            }
            AstNode::Ternary { cond, then_expr, else_expr } => {
                let dest = target.unwrap_or_else(|| self.new_temp());
                let then_ph = self.new_placeholder();
                let else_ph = self.new_placeholder();
                let join_ph = self.new_placeholder();

                self.gen_cond_branch(cond, then_ph, else_ph);

                let then_bb = self.new_bb();
                self.patch_target(then_ph.bb_id, then_bb);
                let _ = self.gen_rvalue(then_expr, Some(dest.clone()));
                self.emit(Quad::Br(join_ph));

                let else_bb = self.new_bb();
                self.patch_target(else_ph.bb_id, else_bb);
                let _ = self.gen_rvalue(else_expr, Some(dest.clone()));
                self.emit(Quad::Br(join_ph));

                let join_bb = self.new_bb();
                self.patch_target(join_ph.bb_id, join_bb);
                ExprResult { op: dest, ty: None }
            }
            AstNode::Str(s) => {
                let idx = self.strings.len() as u32;
                self.strings.push(s.clone());
                let v = Operand::StringLit(idx);
                let dest = target.unwrap_or_else(|| self.new_temp());
                self.emit(Quad::Mov(dest.clone(), v));
                let pty = CType::new(TypeKind::Pointer(Box::new(CType::new(TypeKind::Char(crate::types::Signedness::Default)))));
                ExprResult { op: dest, ty: Some(pty) }
            }
            _ => {
                ExprResult { op: Operand::Const(0), ty: None }
            }
        }
    }

    fn gen_rvalue_typed_only(&self, expr: &AstNode) -> ExprResult {
        match expr {
            AstNode::StabVar { ctype, .. } => {
                ExprResult { op: Operand::Const(0), ty: ctype.clone() }
            }
            AstNode::Deref(inner) => {
                let r = self.gen_rvalue_typed_only(inner);
                let pointee_ty = match r.ty {
                    Some(CType { kind: TypeKind::Pointer(b), .. }) => Some(*b),
                    Some(CType { kind: TypeKind::Array { base, .. }, .. }) => Some(*base),
                    _ => None,
                };
                ExprResult { op: Operand::Const(0), ty: pointee_ty }
            }
            _ => ExprResult { op: Operand::Const(0), ty: None },
        }
    }

    fn gen_arith(&mut self, op: &str, left: &AstNode, right: &AstNode, target: Option<Operand>) -> ExprResult {
        let l = self.gen_rvalue(left, None);
        let r = self.gen_rvalue(right, None);

        let l_is_ptr = is_pointerish(&l.ty);
        let r_is_ptr = is_pointerish(&r.ty);

        match op {
            "+" => {
                if l_is_ptr && !r_is_ptr {
                    let scaled = self.scale_to_pointer(r.op, &l.ty);
                    let dest = target.unwrap_or_else(|| self.new_temp());
                    self.emit(Quad::Add(dest.clone(), l.op, scaled));
                    return ExprResult { op: dest, ty: l.ty };
                } else if r_is_ptr && !l_is_ptr {
                    let scaled = self.scale_to_pointer(l.op, &r.ty);
                    let dest = target.unwrap_or_else(|| self.new_temp());
                    self.emit(Quad::Add(dest.clone(), scaled, r.op));
                    return ExprResult { op: dest, ty: r.ty };
                }
                let dest = target.unwrap_or_else(|| self.new_temp());
                self.emit(Quad::Add(dest.clone(), l.op, r.op));
                ExprResult { op: dest, ty: int_type() }
            }
            "-" => {
                if l_is_ptr && r_is_ptr {
                    let elem_sz = pointee_size(&l.ty, self.struct_defs);
                    let diff = self.new_temp();
                    self.emit(Quad::Sub(diff.clone(), l.op, r.op));
                    let dest = target.unwrap_or_else(|| self.new_temp());
                    self.emit(Quad::Div(dest.clone(), diff, Operand::Const(elem_sz as i64)));
                    return ExprResult { op: dest, ty: int_type() };
                } else if l_is_ptr && !r_is_ptr {
                    let scaled = self.scale_to_pointer(r.op, &l.ty);
                    let dest = target.unwrap_or_else(|| self.new_temp());
                    self.emit(Quad::Sub(dest.clone(), l.op, scaled));
                    return ExprResult { op: dest, ty: l.ty };
                }
                let dest = target.unwrap_or_else(|| self.new_temp());
                self.emit(Quad::Sub(dest.clone(), l.op, r.op));
                ExprResult { op: dest, ty: int_type() }
            }
            "*" => {
                let dest = target.unwrap_or_else(|| self.new_temp());
                self.emit(Quad::Mul(dest.clone(), l.op, r.op));
                ExprResult { op: dest, ty: int_type() }
            }
            "/" => {
                let dest = target.unwrap_or_else(|| self.new_temp());
                self.emit(Quad::Div(dest.clone(), l.op, r.op));
                ExprResult { op: dest, ty: int_type() }
            }
            "%" => {
                let dest = target.unwrap_or_else(|| self.new_temp());
                self.emit(Quad::Mod(dest.clone(), l.op, r.op));
                ExprResult { op: dest, ty: int_type() }
            }
            _ => {
                let dest = target.unwrap_or_else(|| self.new_temp());
                self.emit(Quad::Mov(dest.clone(), l.op));
                ExprResult { op: dest, ty: int_type() }
            }
        }
    }

    fn scale_to_pointer(&mut self, val: Operand, ptr_ty: &Option<CType>) -> Operand {
        let sz = pointee_size(ptr_ty, self.struct_defs);
        if sz == 1 {
            return val;
        }
        let t = self.new_temp();
        self.emit(Quad::Mul(t.clone(), val, Operand::Const(sz as i64)));
        t
    }

    fn gen_unary(&mut self, op: &str, operand: &AstNode, target: Option<Operand>) -> ExprResult {
        match op {
            "-" => {
                let r = self.gen_rvalue(operand, None);
                let dest = target.unwrap_or_else(|| self.new_temp());
                self.emit(Quad::Sub(dest.clone(), Operand::Const(0), r.op));
                ExprResult { op: dest, ty: int_type() }
            }
            "+" => self.gen_rvalue(operand, target),
            "~" => {
                let r = self.gen_rvalue(operand, None);
                let t = self.new_temp();
                self.emit(Quad::Sub(t.clone(), Operand::Const(0), r.op));
                let dest = target.unwrap_or_else(|| self.new_temp());
                self.emit(Quad::Sub(dest.clone(), t, Operand::Const(1)));
                ExprResult { op: dest, ty: int_type() }
            }
            "!" => {
                let r = self.gen_rvalue(operand, None);
                let dest = target.unwrap_or_else(|| self.new_temp());
                let true_ph = self.new_placeholder();
                let false_ph = self.new_placeholder();
                let join_ph = self.new_placeholder();
                self.emit(Quad::Cmp(r.op, Operand::Const(0)));
                self.emit(Quad::BrCC(CC::Eq, true_ph, false_ph));

                let true_bb = self.new_bb();
                self.patch_target(true_ph.bb_id, true_bb);
                self.emit(Quad::Mov(dest.clone(), Operand::Const(1)));
                self.emit(Quad::Br(join_ph));

                let false_bb = self.new_bb();
                self.patch_target(false_ph.bb_id, false_bb);
                self.emit(Quad::Mov(dest.clone(), Operand::Const(0)));
                self.emit(Quad::Br(join_ph));

                let join_bb = self.new_bb();
                self.patch_target(join_ph.bb_id, join_bb);
                ExprResult { op: dest, ty: int_type() }
            }
            "++" | "--" => {
                let amt: i64 = if op == "++" { 1 } else { -1 };
                let lv = self.gen_lvalue(operand);
                match lv {
                    LValue::Direct(d) => {
                        let cur = self.gen_rvalue(operand, None);
                        let scaled = match &cur.ty {
                            Some(t) if matches!(t.kind, TypeKind::Pointer(_)) => {
                                let sz = pointee_size(&cur.ty, self.struct_defs) as i64;
                                Operand::Const(amt * sz)
                            }
                            _ => Operand::Const(amt),
                        };
                        self.emit(Quad::Add(d.clone(), cur.op, scaled));
                        match target {
                            Some(t) => {
                                self.emit(Quad::Mov(t.clone(), d));
                                ExprResult { op: t, ty: cur.ty }
                            }
                            None => ExprResult { op: d, ty: cur.ty },
                        }
                    }
                    LValue::Indirect(p) => {
                        let cur_t = self.new_temp();
                        self.emit(Quad::Load(cur_t.clone(), p.clone()));
                        let new_t = self.new_temp();
                        self.emit(Quad::Add(new_t.clone(), cur_t, Operand::Const(amt)));
                        self.emit(Quad::Store(new_t.clone(), p));
                        match target {
                            Some(t) => {
                                self.emit(Quad::Mov(t.clone(), new_t));
                                ExprResult { op: t, ty: int_type() }
                            }
                            None => ExprResult { op: new_t, ty: int_type() },
                        }
                    }
                }
            }
            _ => self.gen_rvalue(operand, target),
        }
    }

    fn gen_lvalue(&mut self, expr: &AstNode) -> LValue {
        match expr {
            AstNode::StabVar { name, class, .. } => {
                LValue::Direct(Operand::Sym { name: name.clone(), class: class.clone() })
            }
            AstNode::Deref(inner) => {
                let addr = self.gen_rvalue(inner, None);
                LValue::Indirect(addr.op)
            }
            _ => LValue::Direct(Operand::Const(0)),
        }
    }

    fn gen_assign(&mut self, left: &AstNode, right: &AstNode, target: Option<Operand>) -> ExprResult {
        let lv = self.gen_lvalue(left);
        match lv {
            LValue::Direct(d) => {
                let r = self.gen_rvalue(right, Some(d.clone()));
                if let Some(t) = target {
                    self.emit(Quad::Mov(t.clone(), d.clone()));
                    ExprResult { op: t, ty: r.ty }
                } else {
                    ExprResult { op: d, ty: r.ty }
                }
            }
            LValue::Indirect(p) => {
                let r = self.gen_rvalue(right, None);
                self.emit(Quad::Store(r.op.clone(), p));
                if let Some(t) = target {
                    self.emit(Quad::Mov(t.clone(), r.op));
                    ExprResult { op: t, ty: r.ty }
                } else {
                    ExprResult { op: r.op, ty: r.ty }
                }
            }
        }
    }

    fn gen_address(&mut self, inner: &AstNode, target: Option<Operand>) -> ExprResult {
        match inner {
            AstNode::Deref(p) => self.gen_rvalue(p, target),
            AstNode::StabVar { name, class, ctype, .. } => {
                let dest = target.unwrap_or_else(|| self.new_temp());
                let sym = Operand::Sym { name: name.clone(), class: class.clone() };
                self.emit(Quad::Lea(dest.clone(), sym));
                let pty = ctype.clone().map(|t| CType::new(TypeKind::Pointer(Box::new(t))));
                ExprResult { op: dest, ty: pty }
            }
            _ => ExprResult { op: target.unwrap_or(Operand::Const(0)), ty: None },
        }
    }

    fn gen_call(&mut self, func: &AstNode, args: &[AstNode], target: Option<Operand>) -> ExprResult {
        let n_args = args.len();
        for (i, arg) in args.iter().enumerate().rev() {
            let v = self.gen_rvalue(arg, None);
            self.emit(Quad::Arg(i as u32, v.op));
        }

        let (fn_op, ret_ty) = match func {
            AstNode::StabFn { name, ctype, .. } => {
                let ty = ctype.as_ref().and_then(|t| match &t.kind {
                    TypeKind::Function { return_type, .. } => Some((**return_type).clone()),
                    _ => None,
                });
                (Operand::FnAddr(name.clone()), ty)
            }
            other => {
                let r = self.gen_rvalue(other, None);
                let ret_ty = match &r.ty {
                    Some(CType { kind: TypeKind::Pointer(b), .. }) => match &b.kind {
                        TypeKind::Function { return_type, .. } => Some((**return_type).clone()),
                        _ => None,
                    },
                    _ => None,
                };
                (r.op, ret_ty)
            }
        };

        let dest = target.unwrap_or_else(|| self.new_temp());
        self.emit(Quad::Call(Some(dest.clone()), fn_op, n_args as u32));
        ExprResult { op: dest, ty: ret_ty }
    }
}

enum LValue {
    Direct(Operand),
    Indirect(Operand),
}

fn current_terminated(bb: &BasicBlock) -> bool {
    matches!(bb.quads.last(), Some(Quad::Br(_)) | Some(Quad::BrCC(..)) | Some(Quad::Return(_)))
}

fn int_type() -> Option<CType> {
    Some(CType::new(TypeKind::Int(crate::types::Signedness::Default)))
}

fn is_pointerish(ty: &Option<CType>) -> bool {
    match ty {
        Some(CType { kind, .. }) => matches!(kind, TypeKind::Pointer(_) | TypeKind::Array { .. }),
        None => false,
    }
}

fn pointee_size(ty: &Option<CType>, struct_defs: &[StructDef]) -> u32 {
    match ty {
        Some(CType { kind: TypeKind::Pointer(b), .. }) => type_size(b, struct_defs),
        Some(CType { kind: TypeKind::Array { base, .. }, .. }) => type_size(base, struct_defs),
        _ => 1,
    }
}
