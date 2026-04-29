# vibecc

A vibe-coded C compiler. Written in Rust, purely on vibes. Reads
preprocessed C from stdin, emits X86-32 AT&T assembly on stdout.
Pipeline is the textbook one — lexer → parser → AST → quads → codegen.

## Build

```
cargo build
```

Produces `./target/debug/vibecc`.

## Run

The compiler reads preprocessed C from stdin, so use `gcc -E` to expand
includes and macros first. Hand the resulting assembly to gcc with `-m32`
to assemble and link against libc:

```
gcc -E tests/fib.c | ./target/debug/vibecc > out.s
gcc -m32 out.s -o a.out
./a.out
```

A 32-bit toolchain is required (`gcc-multilib` on Debian/Ubuntu, `lib32-gcc-libs`
on Arch).

### Flags

- `--debug` — instead of emitting assembly, dump the intermediate
  representations: declaration listings, AST per function, and quad
  listings.

## How it works

The pipeline runs strictly stage-by-stage; each stage owns one module.

```
stdin (preprocessed C)
   │
   │  lexer.rs       (Lexer::next_token)
   ▼
tokens
   │
   │  parser.rs      (recursive descent; uses symtab.rs + types.rs)
   ▼
AST (ast.rs) + symbol table per function scope
   │
   │  quadgen.rs     (one QuadGen per function)
   ▼
quads (quad.rs): basic blocks of three-address ops
   │
   │  codegen.rs     (per-quad lowering, fixed scratch regs)
   ▼
X86-32 AT&T assembly on stdout
```

### Lexer (`lexer.rs`, `token.rs`)

Hand-written character-by-character lexer. Handles cpp line-marker
directives (`# 12 "file.c"` → file/line tracking), the C keyword set,
identifiers, integer/float literals (including hex floats and octal),
character and string literals with full escape-sequence handling, and
all multi-char operators (`<<=`, `->`, `...`, etc.).

### Parser (`parser.rs`)

Recursive-descent C parser. Notable pieces:

- **Declaration parsing**: declaration specifiers (`DeclSpecs`) accumulate
  type and storage-class info; declarators build a `DeclNode` tree which
  `apply_decl` folds inside-out into a `CType`. This is what lets the
  parser handle declarators like `int (*fn[])(int)`.
- **Symbol table** (`symtab.rs`): a stack of scopes (`Global`, `Function`,
  `Block`, `Prototype`, `StructUnion`). Tags (struct/union) live in a
  separate namespace from ordinary identifiers and skip over struct
  scopes when walking outward. `is_typedef_name` drives the
  typedef-name disambiguation in the lexer-aware sense.
- **Statements & expressions**: full expression precedence climbing,
  C-style operator desugaring (`a[b]` → `*(a+b)`, `->` → indirect select),
  and statement parsing for `if`, `while`, `do/while`, `for`, `switch`,
  `break`, `continue`, `return`, labels, `goto`. C99-style `for (int i=...)`
  opens a stealth block scope.
- **Identifier resolution**: `parse_primary` looks up each identifier in
  the symbol table to emit `StabVar` or `StabFn` AST nodes. Unknown
  identifiers used as call targets get an implicit `extern int name()`
  declaration installed in the global scope.

### AST (`ast.rs`)

Single `AstNode` enum mixing expressions and statements. The printer
(`print_ast`) emits an indented dump used by `--debug`.

### Quad IR (`quad.rs`, `quadgen.rs`)

Each function lowers to a list of basic blocks; each block holds a
sequence of `Quad`s.

- **Operands** (`Operand`): `Const`, `Sym{name, class}` where class is
  `Global` / `Lvar` / `Param(n)`, `Temp(u32)`, `FnAddr(name)`,
  `StringLit(idx)`.
- **Quads**: `Mov`, `Add/Sub/Mul/Div/Mod`, `Lea`, `Load`, `Store`,
  `Cmp`, `Br`, `BrCC(cc, true_target, false_target)`, `Arg(pos, val)`,
  `Call(dest?, fn, n)`, `Return(opt_val)`.
- **Forward jumps via placeholders**: control-flow constructs need to
  branch to a basic block that hasn't been allocated yet. `QuadGen`
  hands out unique placeholder bb_ids, emits the branch, then later
  walks the quad stream and back-patches placeholder targets to real
  bb_ids. This avoids interleaving block allocation with statement
  generation and keeps blocks numbered in source order.
- **Pointer arithmetic**: `gen_arith` checks if either operand is
  pointer-typed; if so, the int operand is multiplied by `sizeof(*p)`
  before the add/sub. Pointer-pointer subtraction divides by
  `sizeof(*p)` after subtracting. Array names decay to pointers via an
  explicit `Lea`.
- **Conditional branches**: `gen_cond_branch` emits `Cmp` plus an
  *inverted* `BrCC` so the natural fall-through is the THEN body, which
  avoids emitting a redundant jump-around for the common case.
- **Strings**: `AstNode::Str` registers the literal in the
  translation-unit string table and emits `Mov %T = StringLit(idx)`.

`--debug` prints the quad IR in a human-readable format (`%T00001`,
`{lvar}`, `.BB1.3`, `BRGE`, etc.).

### Codegen (`codegen.rs`)

Targets X86-32 (cdecl). One pass per function with no register
allocation: every local, parameter, and temporary owns a stack slot,
and `%eax` / `%edx` / `%ecx` are scratch.

- **Frame layout**: locals get negative `%ebp`-relative offsets in
  declaration order. Temporaries each take a 4-byte slot below the
  locals. Frame size is rounded up to 16 bytes. Params live at
  `8(%ebp)`, `12(%ebp)`, … per cdecl.
- **Per-quad lowering**: each quad is lowered ad-hoc. Two memory
  operands aren't allowed in one x86 instruction, so the pattern is
  *load src1 to %eax → op src2 → %eax → store %eax to dst*. `idivl`
  needs `cltd` to sign-extend `%eax` into `%edx` and a non-immediate
  divisor, so `Const` divisors are hoisted into `%ecx` first.
- **Calls**: `Arg` is a `pushl`. `Call` emits `call name`, then
  `addl $4n, %esp` to clean up (caller-cleanup), then stores `%eax` to
  the destination slot.
- **Sections**: `.text` for code, `.section .rodata` + `.string` for
  string literals, `.comm` for uninitialized globals.

## Tests

`tests/` contains end-to-end programs that exercise the major features.
Each one round-trips through `cpp → vibecc → as → ld` and runs:

- `tests/fib.c` — iterative and recursive `fib(n)` for n=0..10, with
  the two implementations cross-checked against each other.
- `tests/bubble.c` — bubble-sorts a 12-element global array in place;
  exercises nested loops and array swaps.
- `tests/primes.c` — Sieve of Eratosthenes over a 100-element global
  array; lots of nested control flow and array indexing.
- `tests/reverse.c` — reverses an 8-element array in place using two
  pointers walking inward (`lo = arr; hi = arr + n - 1; lo < hi`).
- `tests/queens.c` — N-queens via recursive backtracking, run for
  N=1..8 and validated against the OEIS A000170 prefix
  (1, 0, 0, 2, 10, 4, 40, 92). Stresses deep recursion, mutual function
  calls with array-pointer parameters, and block-scoped locals
  (`int got;` declared inside a `for` body).

```
for f in tests/*.c; do
  echo "=== $f ==="
  gcc -E $f | ./target/debug/vibecc > /tmp/out.s
  gcc -m32 /tmp/out.s -o /tmp/a.out
  /tmp/a.out
done
```

## Scope and limitations

- 32-bit only. No `-m64` codegen.
- Integer + pointer types are the working subset. Floats lex and parse
  but don't lower to assembly. No struct member access in codegen
  (the AST and quad IR carry the nodes, but there's no offset
  assignment in lowering).
- No register allocation — every value spills to a stack slot.
- No optimization passes. Quads come out as the parser walks the AST,
  including obviously-redundant moves.
- Initialized globals aren't lowered; uninitialized globals only,
  via `.comm`. Initialized locals work (they desugar to assignment
  statements at point of declaration).
- Function pointers are accepted but only direct calls (`f(...)` where
  `f` is an identifier) are guaranteed to work in codegen.

## Contributors

- [@johnpl-io](https://github.com/johnpl-io)
- Claude (Opus 4.7) — pair-programmed the whole compiler from lexer to
  X86 codegen, on vibes.
