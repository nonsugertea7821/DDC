// Lexer: 入力テキストをトークン列に変換する（Tech-onRust.md §3 参照）

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token {
    // キーワード
    Contract,
    Class,
    Readonly,
    As,
    // セクションヘッダ（コロン込み）
    ReadSection,   // "Read:"
    WriteSection,  // "Write:"
    CallSection,   // "Call:"
    // アノテーション
    KernelAnnotation,  // "[kernel]"
    // 識別子・リテラル
    Ident(String),
    // 区切り
    LParen,   // (
    RParen,   // )
    LBrace,   // {
    RBrace,   // }
    Colon,    // :
    Comma,    // ,
    Dot,      // .
    BracketPair,   // []
    // アクセス修飾子（public / private 等）→ 解析上はスキップするがトークンとして保持
    AccessModifier(String),
    // Rust KW: struct/enum/type/use/fn/impl/pub/mod/trait/extern/const/static/let/type
    RustKeyword(String),
    // その他文字（記号等）
    Other(char),
}

#[derive(Debug, Clone)]
pub(crate) struct Span {
    #[allow(dead_code)]
    pub(crate) line: usize,  // 1-based（将来のエラーレポートで使用）
}

#[derive(Debug, Clone)]
pub(crate) struct Spanned {
    pub(crate) token: Token,
    #[allow(dead_code)]
    pub(crate) span:  Span,
}

// ─── Lexer 本体 ─────────────────────────────────────────────────────────────

pub(crate) struct Lexer<'a> {
    input: &'a str,
    pos:   usize,
    line:  usize,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(input: &'a str) -> Self {
        Self { input, pos: 0, line: 1 }
    }

    /// 全トークンをスキャンして Vec<Spanned> に変換する。
    /// ホワイトスペース（コンマ区切りも含め）はすべてスキップしない — コンマは Comma トークンとして残す。
    pub(crate) fn tokenize(mut self) -> Vec<Spanned> {
        let mut out = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.pos >= self.input.len() {
                break;
            }
            let line = self.line;
            if let Some(tok) = self.next_token() {
                out.push(Spanned { token: tok, span: Span { line } });
            }
        }
        out
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' { self.line += 1; }
        Some(c)
    }

    fn advance_n(&mut self, n: usize) {
        for _ in 0..n { self.advance(); }
    }

    fn starts_with(&self, s: &str) -> bool {
        self.input[self.pos..].starts_with(s)
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // 改行・スペース・タブ
            if matches!(self.peek(), Some(' ' | '\t' | '\r' | '\n')) {
                self.advance();
                continue;
            }
            // ラインコメント //
            if self.starts_with("//") {
                while !matches!(self.peek(), Some('\n') | None) {
                    self.advance();
                }
                continue;
            }
            // ブロックコメント /* */
            if self.starts_with("/*") {
                self.advance_n(2);
                loop {
                    if self.starts_with("*/") {
                        self.advance_n(2);
                        break;
                    }
                    if self.advance().is_none() { break; }
                }
                continue;
            }
            break;
        }
    }

    fn next_token(&mut self) -> Option<Token> {
        let c = self.peek()?;

        // [kernel] または [xxx] — インデックスアクセス [N] はエラー扱いしない（パーサー側で）
        if c == '[' {
            return Some(self.read_bracket());
        }

        // Comma
        if c == ',' { self.advance(); return Some(Token::Comma); }
        if c == '(' { self.advance(); return Some(Token::LParen); }
        if c == ')' { self.advance(); return Some(Token::RParen); }
        if c == '{' { self.advance(); return Some(Token::LBrace); }
        if c == '}' { self.advance(); return Some(Token::RBrace); }
        if c == ':' { self.advance(); return Some(Token::Colon); }
        if c == '.' { self.advance(); return Some(Token::Dot); }

        // 識別子・キーワード
        if c.is_ascii_alphabetic() || c == '_' {
            return Some(self.read_word());
        }

        // その他の記号（Rust body 内の文字はパーサーが一括読み取りするので最後の砦）
        self.advance();
        Some(Token::Other(c))
    }

    /// `[` から始まるブラケット系トークンを読む
    fn read_bracket(&mut self) -> Token {
        self.advance(); // [
        self.skip_whitespace_and_comments();
        if self.peek() == Some(']') {
            self.advance(); // ]
            return Token::BracketPair;
        }
        // [keyword] 形式 → [kernel] など
        let mut ident = String::new();
        while !matches!(self.peek(), Some(']') | None) {
            if let Some(c) = self.advance() {
                ident.push(c);
            }
        }
        if self.peek() == Some(']') { self.advance(); }
        let keyword = ident.trim().to_string();
        if keyword == "kernel" {
            Token::KernelAnnotation
        } else {
            // 不明なアノテーションも Ident として返す
            Token::Ident(format!("[{}]", keyword))
        }
    }

    /// 識別子またはキーワードを読む
    fn read_word(&mut self) -> Token {
        let mut word = String::new();
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
            word.push(self.advance().unwrap());
        }

        // "Read:" / "Write:" / "Call:" — コロン付きで確認
        self.skip_whitespace_and_comments();
        if self.peek() == Some(':') {
            // "::" でなければセクションヘッダ候補
            let is_double_colon = self.input[self.pos..].starts_with("::");
            if !is_double_colon {
                match word.as_str() {
                    "Read"  => { self.advance(); return Token::ReadSection; }
                    "Write" => { self.advance(); return Token::WriteSection; }
                    "Call"  => { self.advance(); return Token::CallSection; }
                    _ => {}
                }
            }
        }

        match word.as_str() {
            "contract" => Token::Contract,
            "class"    => Token::Class,
            "readonly" => Token::Readonly,
            "as"       => Token::As,
            "public" | "private" | "protected" | "internal" => Token::AccessModifier(word),
            "struct" | "enum" | "type" | "use" | "fn" | "impl" | "mod"
            | "trait" | "extern" | "const" | "static" | "let" | "where"
            | "pub" | "crate" | "super" | "self" | "unsafe" | "async" | "await"
            | "return" | "if" | "else" | "match" | "for" | "while" | "loop"
            | "break" | "continue" => Token::RustKeyword(word),
            _ => Token::Ident(word),
        }
    }
}

// ─── ブレース・括弧のバランス読み取りユーティリティ ─────────────────────────

/// `{` の直前の位置から呼ぶ。`{` から対応する `}` までのテキストを返す（両括弧込み）。
/// ネストに対応。
pub(crate) fn read_brace_block(input: &str, start: usize, line: &mut usize) -> Option<(String, usize)> {
    read_balanced(input, start, '{', '}', line)
}

/// `(` の直前の位置から呼ぶ。対応する `)` までのテキストを返す（両括弧込み）。
pub(crate) fn read_paren_block(input: &str, start: usize, line: &mut usize) -> Option<(String, usize)> {
    read_balanced(input, start, '(', ')', line)
}

fn read_balanced(input: &str, start: usize, open: char, close: char, line: &mut usize) -> Option<(String, usize)> {
    if input.as_bytes().get(start)? != &(open as u8) { return None; }
    let mut depth = 0usize;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string = false;
    let mut escape_next = false;
    let chars: Vec<char> = input[start..].chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' { *line += 1; }

        if escape_next { escape_next = false; i += 1; continue; }
        if in_string {
            if c == '\\' { escape_next = true; }
            if c == '"' { in_string = false; }
            i += 1; continue;
        }
        if in_line_comment {
            if c == '\n' { in_line_comment = false; }
            i += 1; continue;
        }
        if in_block_comment {
            if c == '*' && chars.get(i+1) == Some(&'/') { in_block_comment = false; i += 2; continue; }
            i += 1; continue;
        }

        // コメント開始チェック
        if c == '/' && chars.get(i+1) == Some(&'/') { in_line_comment = true; i += 2; continue; }
        if c == '/' && chars.get(i+1) == Some(&'*') { in_block_comment = true; i += 2; continue; }
        if c == '"' { in_string = true; i += 1; continue; }

        if c == open  { depth += 1; }
        if c == close {
            depth -= 1;
            if depth == 0 {
                let end = start + chars[..=i].iter().map(|c| c.len_utf8()).sum::<usize>();
                return Some((input[start..end].to_string(), end));
            }
        }
        i += 1;
    }
    None
}

/// `;` または `{...}` まで Rust テキストを読む（RustPassthrough 用）
/// 戻り値: (テキスト, 終端の次の位置)
pub(crate) fn read_rust_passthrough(input: &str, start: usize, line: &mut usize) -> (String, usize) {
    let mut depth_paren = 0i32;
    let mut depth_angle = 0i32;
    let chars: Vec<char> = input[start..].chars().collect();
    let mut i = 0usize;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while i < chars.len() {
        let c = chars[i];
        if c == '\n' { *line += 1; }

        if in_line_comment {
            if c == '\n' { in_line_comment = false; }
            i += 1; continue;
        }
        if in_block_comment {
            if c == '*' && chars.get(i+1) == Some(&'/') { in_block_comment = false; i += 2; continue; }
            i += 1; continue;
        }
        if c == '/' && chars.get(i+1) == Some(&'/') { in_line_comment = true; i += 2; continue; }
        if c == '/' && chars.get(i+1) == Some(&'*') { in_block_comment = true; i += 2; continue; }

        if c == '<' { depth_angle += 1; i += 1; continue; }
        if c == '>' { depth_angle -= 1; if depth_angle < 0 { depth_angle = 0; } i += 1; continue; }
        if c == '(' { depth_paren += 1; i += 1; continue; }
        if c == ')' { depth_paren -= 1; i += 1; continue; }

        if depth_angle == 0 && depth_paren == 0 {
            if c == ';' {
                let end_off = chars[..=i].iter().map(|c| c.len_utf8()).sum::<usize>();
                let end = start + end_off;
                return (input[start..end].to_string(), end);
            }
            if c == '{' {
                // ブレースブロックまで一括読み取り
                let abs = start + chars[..i].iter().map(|c| c.len_utf8()).sum::<usize>();
                if let Some((_blk, end)) = read_brace_block(input, abs, line) {
                    return (input[start..end].to_string(), end);
                }
                break;
            }
        }
        i += 1;
    }
    // ファイル末尾まで
    let end = start + chars.iter().map(|c| c.len_utf8()).sum::<usize>();
    (input[start..end].to_string(), end)
}
