use crate::token::{IntSize, NumberVal, Token, TokenKind};
use std::iter::Peekable;
use std::str::Chars;

pub struct Lexer<'a> {
    input: Peekable<Chars<'a>>,
    filename: String,
    line: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer {
            input: input.chars().peekable(),
            filename: "<stdin>".to_string(),
            line: 1,
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.input.peek().copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.input.next();
        if c == Some('\n') {
            self.line += 1;
        }
        c
    }

    fn cur_loc(&self) -> (String, u32) {
        (self.filename.clone(), self.line)
    }

    fn parse_line_marker(&mut self) {
        let mut num_str = String::new();
        while self.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            num_str.push(self.advance().unwrap());
        }
        while self.peek() == Some(' ') {
            self.advance();
        }
        if self.peek() == Some('"') {
            self.advance();
            let mut fname = String::new();
            loop {
                match self.advance() {
                    Some('"') | None => break,
                    Some(c) => fname.push(c),
                }
            }
            self.filename = fname;
        }
        while self.peek() != Some('\n') && self.peek().is_some() {
            self.advance();
        }
        if let Ok(n) = num_str.parse::<u32>() {
            self.line = n.saturating_sub(1);
        }
    }

    fn skip_block_comment(&mut self) {
        loop {
            match self.advance() {
                None => break,
                Some('*') => {
                    if self.peek() == Some('/') {
                        self.advance();
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    fn skip_line_comment(&mut self) {
        loop {
            match self.peek() {
                None | Some('\n') => break,
                _ => { self.advance(); }
            }
        }
    }

    fn parse_escape_char(&mut self, first: char) -> u8 {
        match first {
            'n' => b'\n',
            't' => b'\t',
            'r' => b'\r',
            'b' => b'\x08',
            'f' => b'\x0C',
            'v' => b'\x0B',
            'a' => b'\x07',
            '\\' => b'\\',
            '\'' => b'\'',
            '"' => b'"',
            '?' => b'?',
            '0'..='7' => {
                let mut val = (first as u8 - b'0') as u32;
                for _ in 0..2 {
                    match self.peek() {
                        Some(c @ '0'..='7') => {
                            val = val * 8 + (c as u32 - '0' as u32);
                            self.advance();
                        }
                        _ => break,
                    }
                }
                (val & 0xFF) as u8
            }
            'x' => {
                let mut val: u64 = 0;
                let mut count = 0;
                let mut all_hex = String::new();
                while self.peek().map(|c| c.is_ascii_hexdigit()).unwrap_or(false) {
                    let c = self.advance().unwrap();
                    all_hex.push(c);
                    val = val * 16 + c.to_digit(16).unwrap() as u64;
                    count += 1;
                }
                if count == 0 {
                    eprintln!(
                        "{}:{}:Error:Invalid hex escape",
                        self.filename, self.line
                    );
                    return 0;
                }
                if val > 0xFF {
                    eprintln!(
                        "{}:{}:Warning:Hex escape sequence \\x{} out of range",
                        self.filename, self.line, all_hex
                    );
                    return 0xFF;
                }
                val as u8
            }
            other => {
                eprintln!(
                    "{}:{}:Warning:Unknown escape sequence '\\{}'",
                    self.filename, self.line, other
                );
                other as u8
            }
        }
    }

    fn read_char_lit(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            match self.advance() {
                None | Some('\n') => {
                    eprintln!("{}:{}:Error:Unterminated character literal", self.filename, self.line);
                    break;
                }
                Some('\'') => break,
                Some('\\') => {
                    let c = match self.advance() {
                        Some(c) => c,
                        None => break,
                    };
                    bytes.push(self.parse_escape_char(c));
                }
                Some(c) => bytes.push(c as u8),
            }
        }
        if bytes.len() > 1 {
            eprintln!(
                "{}:{}:Warning:Unsupported multibyte character literal truncated to first byte",
                self.filename, self.line
            );
            bytes.truncate(1);
        }
        bytes
    }

    fn read_string_lit(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            match self.advance() {
                None | Some('\n') => {
                    eprintln!("{}:{}:Error:Unterminated string literal", self.filename, self.line);
                    break;
                }
                Some('"') => break,
                Some('\\') => {
                    let c = match self.advance() {
                        Some(c) => c,
                        None => break,
                    };
                    bytes.push(self.parse_escape_char(c));
                }
                Some(c) => bytes.push(c as u8),
            }
        }
        bytes
    }

    fn read_number(&mut self, first: char) -> TokenKind {
        let mut s = String::new();
        s.push(first);

        if first == '0' && (self.peek() == Some('x') || self.peek() == Some('X')) {
            s.push(self.advance().unwrap());
            while self.peek().map(|c| c.is_ascii_hexdigit()).unwrap_or(false) {
                s.push(self.advance().unwrap());
            }
            if self.peek() == Some('.') {
                s.push(self.advance().unwrap());
                while self.peek().map(|c| c.is_ascii_hexdigit()).unwrap_or(false) {
                    s.push(self.advance().unwrap());
                }
            }
            if self.peek() == Some('p') || self.peek() == Some('P') {
                s.push(self.advance().unwrap());
                if self.peek() == Some('+') || self.peek() == Some('-') {
                    s.push(self.advance().unwrap());
                }
                while self.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    s.push(self.advance().unwrap());
                }
                return self.finish_real_number_hex(s);
            }
            return self.finish_int_number(s, 16);
        }

        if first == '0' {
            while self.peek().map(|c| matches!(c, '0'..='7')).unwrap_or(false) {
                s.push(self.advance().unwrap());
            }
            if self.peek() == Some('.') {
                s.push(self.advance().unwrap());
                while self.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    s.push(self.advance().unwrap());
                }
                if self.peek() == Some('e') || self.peek() == Some('E') {
                    s.push(self.advance().unwrap());
                    if self.peek() == Some('+') || self.peek() == Some('-') {
                        s.push(self.advance().unwrap());
                    }
                    while self.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                        s.push(self.advance().unwrap());
                    }
                }
                return self.finish_real_number(s);
            }
            if self.peek() == Some('e') || self.peek() == Some('E') {
                s.push(self.advance().unwrap());
                if self.peek() == Some('+') || self.peek() == Some('-') {
                    s.push(self.advance().unwrap());
                }
                while self.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    s.push(self.advance().unwrap());
                }
                return self.finish_real_number(s);
            }
            let base = if s.len() > 1 { 8 } else { 10 };
            return self.finish_int_number(s, base);
        }

        while self.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            s.push(self.advance().unwrap());
        }
        if self.peek() == Some('.') {
            s.push(self.advance().unwrap());
            while self.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                s.push(self.advance().unwrap());
            }
            if self.peek() == Some('e') || self.peek() == Some('E') {
                s.push(self.advance().unwrap());
                if self.peek() == Some('+') || self.peek() == Some('-') {
                    s.push(self.advance().unwrap());
                }
                while self.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    s.push(self.advance().unwrap());
                }
            }
            return self.finish_real_number(s);
        }
        if self.peek() == Some('e') || self.peek() == Some('E') {
            s.push(self.advance().unwrap());
            if self.peek() == Some('+') || self.peek() == Some('-') {
                s.push(self.advance().unwrap());
            }
            while self.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                s.push(self.advance().unwrap());
            }
            return self.finish_real_number(s);
        }
        self.finish_int_number(s, 10)
    }

    fn finish_real_number(&mut self, mut s: String) -> TokenKind {
        let float_size = self.consume_float_suffix(&mut s);
        let parse_str: String = s.chars().filter(|c| !matches!(c, 'f'|'F'|'l'|'L')).collect();
        let val = parse_str.parse::<f64>().unwrap_or(0.0);
        let display = format_g(val);
        self.make_real_token(val, display, float_size)
    }

    fn finish_real_number_hex(&mut self, mut s: String) -> TokenKind {
        let float_size = self.consume_float_suffix(&mut s);
        let val = parse_hex_float(&s);
        let display = format_g(val);
        self.make_real_token(val, display, float_size)
    }

    fn consume_float_suffix(&mut self, s: &mut String) -> FloatSize {
        if self.peek() == Some('f') || self.peek() == Some('F') {
            s.push(self.advance().unwrap());
            FloatSize::Float
        } else if self.peek() == Some('l') || self.peek() == Some('L') {
            s.push(self.advance().unwrap());
            FloatSize::LongDouble
        } else {
            FloatSize::Double
        }
    }

    fn make_real_token(&self, val: f64, display: String, float_size: FloatSize) -> TokenKind {
        TokenKind::Number(NumberVal {
            is_real: true,
            int_val: 0,
            real_val: val,
            real_str: display,
            unsigned: false,
            size: match float_size {
                FloatSize::Float => IntSize::Int,
                FloatSize::Double => IntSize::Long,
                FloatSize::LongDouble => IntSize::LongLong,
            },
        })
    }

    fn finish_int_number(&mut self, s: String, base: u32) -> TokenKind {
        let mut unsigned = false;
        let mut long_count = 0u32;

        loop {
            match self.peek() {
                Some('u') | Some('U') if !unsigned => {
                    unsigned = true;
                    self.advance();
                }
                Some('l') | Some('L') if long_count < 2 => {
                    self.advance();
                    if long_count == 0 {
                        if self.peek() == Some('l') || self.peek() == Some('L') {
                            self.advance();
                            long_count = 2;
                        } else {
                            long_count = 1;
                        }
                    } else {
                        long_count += 1;
                    }
                }
                _ => break,
            }
        }

        let val = parse_int_literal(&s, base);

        TokenKind::Number(NumberVal {
            is_real: false,
            int_val: val,
            real_val: 0.0,
            real_str: String::new(),
            unsigned,
            size: match long_count {
                0 => IntSize::Int,
                1 => IntSize::Long,
                _ => IntSize::LongLong,
            },
        })
    }

    pub fn next_token(&mut self) -> Token {
        loop {
            while self.peek().map(|c| c == ' ' || c == '\t' || c == '\r').unwrap_or(false) {
                self.advance();
            }

            let (filename, line) = self.cur_loc();

            match self.peek() {
                None => {
                    return Token { kind: TokenKind::Eof, filename, line };
                }
                Some('\n') => {
                    self.advance();
                    continue;
                }
                Some('#') => {
                    self.advance();
                    while self.peek() == Some(' ') { self.advance(); }
                    if self.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                        self.parse_line_marker();
                    } else {
                        while self.peek() != Some('\n') && self.peek().is_some() {
                            self.advance();
                        }
                    }
                    continue;
                }
                Some('/') => {
                    self.advance();
                    match self.peek() {
                        Some('/') => { self.advance(); self.skip_line_comment(); continue; }
                        Some('*') => { self.advance(); self.skip_block_comment(); continue; }
                        Some('=') => { self.advance(); return Token { kind: TokenKind::Diveq, filename, line }; }
                        _ => return Token { kind: TokenKind::Char('/'), filename, line },
                    }
                }
                _ => {}
            }

            let c = self.advance().unwrap();

            let kind = match c {
                '"' => {
                    let bytes = self.read_string_lit();
                    TokenKind::Str(bytes)
                }
                '\'' => {
                    let bytes = self.read_char_lit();
                    TokenKind::CharLit(bytes)
                }
                '0'..='9' => self.read_number(c),
                'a'..='z' | 'A'..='Z' | '_' => {
                    let mut ident = String::new();
                    ident.push(c);
                    while self.peek().map(|c| c.is_alphanumeric() || c == '_').unwrap_or(false) {
                        ident.push(self.advance().unwrap());
                    }
                    keyword_or_ident(ident)
                }
                '-' => match self.peek() {
                    Some('>') => { self.advance(); TokenKind::Indsel }
                    Some('-') => { self.advance(); TokenKind::Minusminus }
                    Some('=') => { self.advance(); TokenKind::Minuseq }
                    _ => TokenKind::Char('-'),
                },
                '+' => match self.peek() {
                    Some('+') => { self.advance(); TokenKind::Plusplus }
                    Some('=') => { self.advance(); TokenKind::Pluseq }
                    _ => TokenKind::Char('+'),
                },
                '<' => match self.peek() {
                    Some('<') => {
                        self.advance();
                        if self.peek() == Some('=') { self.advance(); TokenKind::Shleq }
                        else { TokenKind::Shl }
                    }
                    Some('=') => { self.advance(); TokenKind::Lteq }
                    _ => TokenKind::Char('<'),
                },
                '>' => match self.peek() {
                    Some('>') => {
                        self.advance();
                        if self.peek() == Some('=') { self.advance(); TokenKind::Shreq }
                        else { TokenKind::Shr }
                    }
                    Some('=') => { self.advance(); TokenKind::Gteq }
                    _ => TokenKind::Char('>'),
                },
                '=' => match self.peek() {
                    Some('=') => { self.advance(); TokenKind::Eqeq }
                    _ => TokenKind::Char('='),
                },
                '!' => match self.peek() {
                    Some('=') => { self.advance(); TokenKind::Noteq }
                    _ => TokenKind::Char('!'),
                },
                '&' => match self.peek() {
                    Some('&') => { self.advance(); TokenKind::Logand }
                    Some('=') => { self.advance(); TokenKind::Andeq }
                    _ => TokenKind::Char('&'),
                },
                '|' => match self.peek() {
                    Some('|') => { self.advance(); TokenKind::Logor }
                    Some('=') => { self.advance(); TokenKind::Oreq }
                    _ => TokenKind::Char('|'),
                },
                '*' => match self.peek() {
                    Some('=') => { self.advance(); TokenKind::Timeseq }
                    _ => TokenKind::Char('*'),
                },
                '%' => match self.peek() {
                    Some('=') => { self.advance(); TokenKind::Modeq }
                    _ => TokenKind::Char('%'),
                },
                '^' => match self.peek() {
                    Some('=') => { self.advance(); TokenKind::Xoreq }
                    _ => TokenKind::Char('^'),
                },
                '.' => {
                    if self.peek() == Some('.') {
                        self.advance();
                        if self.peek() == Some('.') {
                            self.advance();
                            TokenKind::Ellipsis
                        } else {
                            TokenKind::Char('.')
                        }
                    } else if self.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                        let mut s = String::from("0.");
                        while self.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                            s.push(self.advance().unwrap());
                        }
                        if self.peek() == Some('e') || self.peek() == Some('E') {
                            s.push(self.advance().unwrap());
                            if self.peek() == Some('+') || self.peek() == Some('-') {
                                s.push(self.advance().unwrap());
                            }
                            while self.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                                s.push(self.advance().unwrap());
                            }
                        }
                        self.finish_real_number(s)
                    } else {
                        TokenKind::Char('.')
                    }
                }
                '[' | ']' | '{' | '}' | '(' | ')' | ';' | ':' | ',' | '~' | '?' => {
                    TokenKind::Char(c)
                }
                other => {
                    eprintln!(
                        "{}:{}:Error:Unrecognized character '{}'",
                        filename, line, other
                    );
                    continue;
                }
            };

            return Token { kind, filename, line };
        }
    }
}

enum FloatSize { Float, Double, LongDouble }

fn parse_int_literal(s: &str, base: u32) -> u64 {
    let digits = match base {
        16 => &s[2..],
        8 if s.len() > 1 => &s[1..],
        _ => s,
    };
    u64::from_str_radix(digits, base).unwrap_or(0)
}

fn parse_hex_float(s: &str) -> f64 {
    let s = s.trim_end_matches(|c| matches!(c, 'f'|'F'|'l'|'L'));
    let s = if s.starts_with("0x") || s.starts_with("0X") { &s[2..] } else { s };

    let (mantissa_str, exp_str) = if let Some(p) = s.find(|c| c == 'p' || c == 'P') {
        (&s[..p], &s[p+1..])
    } else {
        (s, "0")
    };

    let exp: i32 = exp_str.parse().unwrap_or(0);

    let (int_part, frac_part) = if let Some(d) = mantissa_str.find('.') {
        (&mantissa_str[..d], &mantissa_str[d+1..])
    } else {
        (mantissa_str, "")
    };

    let int_val = u64::from_str_radix(int_part, 16).unwrap_or(0) as f64;
    let frac_val = if !frac_part.is_empty() {
        let frac_int = u64::from_str_radix(frac_part, 16).unwrap_or(0) as f64;
        frac_int / 16f64.powi(frac_part.len() as i32)
    } else {
        0.0
    };

    (int_val + frac_val) * 2f64.powi(exp)
}

fn format_g(val: f64) -> String {
    if val == 0.0 {
        return "0".to_string();
    }
    let s = format!("{:.6e}", val);
    let parts: Vec<&str> = s.split('e').collect();
    if parts.len() != 2 {
        return format!("{}", val);
    }
    let exp: i32 = parts[1].parse().unwrap_or(0);

    if exp >= -4 && exp < 6 {
        let precision = (5 - exp).max(0) as usize;
        let fixed = format!("{:.prec$}", val, prec = precision);
        if fixed.contains('.') {
            let trimmed = fixed.trim_end_matches('0').trim_end_matches('.');
            trimmed.to_string()
        } else {
            fixed
        }
    } else {
        let mant_str = parts[0];
        let mant = if mant_str.contains('.') {
            mant_str.trim_end_matches('0').trim_end_matches('.')
        } else {
            mant_str
        };
        let exp_str = if exp >= 0 {
            format!("e+{:02}", exp)
        } else {
            format!("e-{:02}", -exp)
        };
        format!("{}{}", mant, exp_str)
    }
}

fn keyword_or_ident(s: String) -> TokenKind {
    match s.as_str() {
        "auto" => TokenKind::Auto,
        "break" => TokenKind::Break,
        "case" => TokenKind::Case,
        "char" => TokenKind::KwChar,
        "const" => TokenKind::Const,
        "continue" => TokenKind::Continue,
        "default" => TokenKind::Default,
        "do" => TokenKind::Do,
        "double" => TokenKind::Double,
        "else" => TokenKind::Else,
        "enum" => TokenKind::Enum,
        "extern" => TokenKind::Extern,
        "float" => TokenKind::Float,
        "for" => TokenKind::For,
        "goto" => TokenKind::Goto,
        "if" => TokenKind::If,
        "inline" => TokenKind::Inline,
        "int" => TokenKind::Int,
        "long" => TokenKind::Long,
        "register" => TokenKind::Register,
        "restrict" => TokenKind::Restrict,
        "return" => TokenKind::Return,
        "short" => TokenKind::Short,
        "signed" => TokenKind::Signed,
        "sizeof" => TokenKind::Sizeof,
        "static" => TokenKind::Static,
        "struct" => TokenKind::Struct,
        "switch" => TokenKind::Switch,
        "typedef" => TokenKind::Typedef,
        "union" => TokenKind::Union,
        "unsigned" => TokenKind::Unsigned,
        "void" => TokenKind::Void,
        "volatile" => TokenKind::Volatile,
        "while" => TokenKind::While,
        "_Bool" => TokenKind::Bool,
        "_Complex" => TokenKind::Complex,
        "_Imaginary" => TokenKind::Imaginary,
        _ => TokenKind::Ident(s),
    }
}
