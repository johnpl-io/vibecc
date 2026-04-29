mod token;
mod lexer;
mod types;
mod symtab;
mod parser;
mod printer;
mod ast;
mod quad;
mod quadgen;
mod codegen;

use lexer::Lexer;
use token::TokenKind;
use std::io::{self, Read};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    let debug = args.iter().any(|a| a == "--debug");

    let mut input = String::new();
    io::stdin().read_to_string(&mut input).expect("Failed to read stdin");

    let mut lex = Lexer::new(&input);
    let mut tokens = Vec::new();
    let mut file_name = String::from("stdin");
    loop {
        let tok = lex.next_token();
        if tok.kind == TokenKind::Eof {
            break;
        }
        if file_name == "stdin" && !tok.filename.is_empty() {
            file_name = tok.filename.clone();
        }
        tokens.push(tok);
    }

    let mut parser = parser::Parser::new(tokens);
    parser.debug = debug;
    parser.parse_translation_unit();

    if !debug {
        let mut out = String::new();
        codegen::emit_translation_unit(
            &mut out,
            &parser.globals,
            &parser.strings,
            &parser.functions,
            &file_name,
        );
        print!("{}", out);
    }
}
