// パーサー本体（Tech-onRust.md §3, §4, §5 参照）

use crate::{
    ast::*,
    error::ParseError,
    lexer::{
        Lexer, Spanned, Token,
        read_brace_block, read_paren_block, read_rust_passthrough,
    },
};

// ─── 公開エントリーポイント ──────────────────────────────────────────────────

pub fn parse_ddc_file(input: &str) -> Result<DdcFile, ParseError> {
    Parser::new(input).parse()
}

// ─── Parser 状態 ─────────────────────────────────────────────────────────────

struct Parser<'a> {
    input:   &'a str,
    tokens:  Vec<Spanned>,
    pos:     usize,
    raw_pos: usize,
    raw_line: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        let tokens = Lexer::new(input).tokenize();
        Self { input, tokens, pos: 0, raw_pos: 0, raw_line: 1 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|s| &s.token)
    }

    fn advance(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).map(|s| s.token.clone());
        if t.is_some() { self.pos += 1; }
        t
    }

    /// raw_pos 以降の入力で最初にマッチするトークンへ token カーソルを進める
    fn align_token_pos_to_raw(&mut self) {
        while self.pos < self.tokens.len() {
            let tok = &self.tokens[self.pos];
            let needle = token_text(&tok.token);
            if let Some(off) = self.input[self.raw_pos..].find(needle.as_str()) {
                if off == 0 { break; }
            }
            self.pos += 1;
        }
    }

    // ─── parse 本体 ──────────────────────────────────────────────────────────

    fn parse(mut self) -> Result<DdcFile, ParseError> {
        let mut items = Vec::new();
        let mut raw_pos = 0usize;
        let mut raw_line = 1usize;

        while raw_pos < self.input.len() {
            raw_pos = skip_whitespace_and_comments(self.input, raw_pos, &mut raw_line);
            if raw_pos >= self.input.len() { break; }

            self.raw_pos = raw_pos;
            self.raw_line = raw_line;
            self.align_token_pos_to_raw();

            let (item, new_raw_pos, new_raw_line) =
                self.parse_item(raw_pos, raw_line)?;
            items.push(item);
            raw_pos = new_raw_pos;
            raw_line = new_raw_line;
        }

        Ok(DdcFile { items })
    }

    fn parse_item(
        &mut self,
        raw_pos: usize,
        raw_line: usize,
    ) -> Result<(DdcItem, usize, usize), ParseError> {
        match self.peek() {
            Some(Token::Contract) => self.parse_contract_decl(raw_pos, raw_line),
            Some(Token::Class)    => self.parse_impl_decl(raw_pos, raw_line),
            Some(Token::RustKeyword(_)) => {
                let (text, end, end_line) =
                    self.read_rust_passthrough_item(raw_pos, raw_line);
                Ok((DdcItem::RustPassthrough(text), end, end_line))
            }
            _ => {
                let (func, end, end_line) =
                    self.parse_ddc_fn(raw_pos, raw_line)?;
                Ok((DdcItem::DdcFn(func), end, end_line))
            }
        }
    }

    // ─── RustPassthrough ─────────────────────────────────────────────────────

    fn read_rust_passthrough_item(
        &mut self,
        raw_pos: usize,
        raw_line: usize,
    ) -> (String, usize, usize) {
        let mut line = raw_line;
        let (text, end) = read_rust_passthrough(self.input, raw_pos, &mut line);
        (text, end, line)
    }

    // ─── DdcFn ───────────────────────────────────────────────────────────────

    fn parse_ddc_fn(
        &mut self,
        raw_pos: usize,
        raw_line: usize,
    ) -> Result<(DdcFn, usize, usize), ParseError> {
        let mut rpos = raw_pos;
        let mut rline = raw_line;

        // 1. [kernel] アノテーション
        let mut annotations = Vec::new();
        loop {
            rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);
            if self.input[rpos..].starts_with("[kernel]") {
                annotations.push(DdcAnnotation::Kernel);
                rpos += "[kernel]".len();
            } else {
                break;
            }
        }
        self.raw_pos = rpos;
        self.align_token_pos_to_raw();

        // 2. アクセス修飾子スキップ
        while matches!(self.peek(), Some(Token::AccessModifier(_))) {
            self.advance();
        }
        rpos = self.find_raw_pos_after_tokens(rpos);

        // 3. readonly フラグ
        let is_readonly = matches!(self.peek(), Some(Token::Readonly));
        if is_readonly {
            self.advance();
            rpos = self.find_raw_pos_after_tokens(rpos);
        }

        // 4. シグネチャ
        rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);
        let (sig, after_sig) = self.parse_ddc_fn_sig(rpos, rline)?;
        rpos = after_sig;

        // 5. 契約ブロック ( ... ) — 省略可能
        rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);
        let (contract, has_contract_block) = if self.input[rpos..].starts_with('(') {
            let (paren_text, after_paren) =
                read_paren_block(self.input, rpos, &mut rline)
                    .ok_or_else(|| ParseError {
                        message: "契約ブロックの ')' が見つかりません".into(),
                        line: Some(rline),
                    })?;
            rpos = after_paren;
            (parse_contract_block(&paren_text, rline)?, true)
        } else {
            (DeclaredContract::default(), false)
        };

        // 6. body { ... }
        rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);
        if !self.input[rpos..].starts_with('{') {
            return Err(ParseError {
                message: "関数 body の '{' が見つかりません".into(),
                line: Some(rline),
            });
        }
        let (body_text, after_body) =
            read_brace_block(self.input, rpos, &mut rline)
                .ok_or_else(|| ParseError {
                    message: "関数 body の '}' が見つかりません".into(),
                    line: Some(rline),
                })?;
        rpos = after_body;

        let id = sig.name.clone();
        let func = DdcFn { annotations, id, is_readonly, has_contract_block, sig, contract, body: DdcBody::Rust(body_text) };
        Ok((func, rpos, rline))
    }

    fn parse_ddc_fn_sig(
        &mut self,
        sig_start: usize,
        start_line: usize,
    ) -> Result<(DdcFnSig, usize), ParseError> {
        let mut rpos = sig_start;
        let mut rline = start_line;

        // 戻り型
        let (return_type, after_ret) = read_type_expr(self.input, rpos, &mut rline)
            .ok_or_else(|| ParseError {
                message: "戻り型の読み取りに失敗しました".into(),
                line: Some(rline),
            })?;
        rpos = after_ret;
        rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);

        // 関数名
        let (name, after_name) = read_ident(self.input, rpos)
            .ok_or_else(|| ParseError {
                message: "関数名の読み取りに失敗しました".into(),
                line: Some(rline),
            })?;
        rpos = after_name;
        rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);

        // パラメータリスト (...)
        if !self.input[rpos..].starts_with('(') {
            return Err(ParseError {
                message: "パラメータリストの '(' が見つかりません".into(),
                line: Some(rline),
            });
        }
        let (paren_text, after_params) =
            read_paren_block(self.input, rpos, &mut rline)
                .ok_or_else(|| ParseError {
                    message: "パラメータリストの ')' が見つかりません".into(),
                    line: Some(rline),
                })?;
        let params = parse_params(&paren_text[1..paren_text.len()-1], rline)?;
        rpos = after_params;

        Ok((DdcFnSig { return_type, name: FunctionId(name), params }, rpos))
    }

    /// アクセス修飾子や readonly を消費した後、次のトークン手前の raw_pos を推定する
    fn find_raw_pos_after_tokens(&self, base: usize) -> usize {
        if let Some(tok) = self.tokens.get(self.pos) {
            let needle = token_text(&tok.token);
            if let Some(off) = self.input[base..].find(needle.as_str()) {
                return base + off;
            }
        }
        base
    }

    // ─── ContractDecl ─────────────────────────────────────────────────────────

    fn parse_contract_decl(
        &mut self,
        raw_pos: usize,
        raw_line: usize,
    ) -> Result<(DdcItem, usize, usize), ParseError> {
        let mut rpos = raw_pos;
        let mut rline = raw_line;

        rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);
        rpos += "contract".len();
        self.advance(); // Contract

        rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);
        let (type_name, after_name) = read_ident(self.input, rpos)
            .ok_or_else(|| ParseError { message: "contract 名の読み取りに失敗".into(), line: Some(rline) })?;
        rpos = after_name;
        rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);

        let envelope = if self.input[rpos..].starts_with('(') {
            let (paren_text, after_paren) =
                read_paren_block(self.input, rpos, &mut rline)
                    .ok_or_else(|| ParseError { message: "contract envelope ')' が見つかりません".into(), line: Some(rline) })?;
            rpos = after_paren;
            parse_contract_block(&paren_text, rline)?
        } else {
            DeclaredContract::default()
        };

        rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);
        let (brace_text, after_brace) =
            read_brace_block(self.input, rpos, &mut rline)
                .ok_or_else(|| ParseError { message: "contract 本体 '}' が見つかりません".into(), line: Some(rline) })?;
        let methods = parse_contract_methods(&brace_text[1..brace_text.len()-1], rline)?;
        rpos = after_brace;

        Ok((DdcItem::Contract(ContractDecl {
            id: TypeId(type_name),
            envelope,
            methods,
        }), rpos, rline))
    }

    // ─── ImplDecl ────────────────────────────────────────────────────────────

    fn parse_impl_decl(
        &mut self,
        raw_pos: usize,
        raw_line: usize,
    ) -> Result<(DdcItem, usize, usize), ParseError> {
        let mut rpos = raw_pos;
        let mut rline = raw_line;

        rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);
        rpos += "class".len();
        self.advance(); // Class

        rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);
        let (type_name, after_type) = read_ident(self.input, rpos)
            .ok_or_else(|| ParseError { message: "class 型名読み取り失敗".into(), line: Some(rline) })?;
        rpos = after_type;
        rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);

        if !self.input[rpos..].starts_with(':') {
            return Err(ParseError { message: "class 宣言の ':' が見つかりません".into(), line: Some(rline) });
        }
        rpos += 1;
        rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);

        let mut contract_ids = Vec::new();
        loop {
            let (name, after_name) = read_ident(self.input, rpos)
                .ok_or_else(|| ParseError { message: "contract 名読み取り失敗".into(), line: Some(rline) })?;
            contract_ids.push(TypeId(name));
            rpos = after_name;
            rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline);
            if self.input[rpos..].starts_with(',') { rpos += 1; rpos = skip_whitespace_and_comments(self.input, rpos, &mut rline); } else { break; }
        }

        let (brace_text, after_brace) =
            read_brace_block(self.input, rpos, &mut rline)
                .ok_or_else(|| ParseError { message: "class 本体 '}' が見つかりません".into(), line: Some(rline) })?;
        let methods = parse_impl_methods(&brace_text[1..brace_text.len()-1], rline)?;
        rpos = after_brace;

        Ok((DdcItem::Impl(ImplDecl {
            type_id: TypeId(type_name),
            contract_ids,
            methods,
        }), rpos, rline))
    }
}

// ─── テキストレベルのユーティリティ ──────────────────────────────────────────

pub(crate) fn skip_whitespace_and_comments(input: &str, mut pos: usize, line: &mut usize) -> usize {
    loop {
        if pos >= input.len() { return pos; }
        let c = input[pos..].chars().next().unwrap();
        if matches!(c, ' ' | '\t' | '\r') { pos += c.len_utf8(); continue; }
        if c == '\n' { *line += 1; pos += 1; continue; }
        if input[pos..].starts_with("//") {
            while pos < input.len() && input.as_bytes()[pos] != b'\n' { pos += 1; }
            continue;
        }
        if input[pos..].starts_with("/*") {
            pos += 2;
            loop {
                if pos + 1 < input.len() && &input[pos..pos+2] == "*/" { pos += 2; break; }
                if input.as_bytes().get(pos) == Some(&b'\n') { *line += 1; }
                pos += 1;
                if pos >= input.len() { break; }
            }
            continue;
        }
        return pos;
    }
}

pub(crate) fn read_ident(input: &str, pos: usize) -> Option<(String, usize)> {
    let first = input[pos..].chars().next()?;
    if !first.is_ascii_alphabetic() && first != '_' { return None; }
    let mut end = pos + first.len_utf8();
    for c in input[end..].chars() {
        if c.is_ascii_alphanumeric() || c == '_' { end += c.len_utf8(); } else { break; }
    }
    Some((input[pos..end].to_string(), end))
}

pub(crate) fn read_type_expr(input: &str, pos: usize, line: &mut usize) -> Option<(String, usize)> {
    let (_, after_ident) = read_ident(input, pos)?;
    let mut end = after_ident;
    if input[end..].starts_with('<') {
        let mut depth = 0i32;
        let chars: Vec<char> = input[end..].chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '\n' { *line += 1; }
            if c == '<' { depth += 1; }
            if c == '>' {
                depth -= 1;
                if depth == 0 {
                    end += chars[..=i].iter().map(|c| c.len_utf8()).sum::<usize>();
                    break;
                }
            }
            i += 1;
        }
    }
    Some((input[pos..end].to_string(), end))
}

fn token_text(tok: &Token) -> String {
    match tok {
        Token::Contract          => "contract".into(),
        Token::Class             => "class".into(),
        Token::Readonly          => "readonly".into(),
        Token::As                => "as".into(),
        Token::ReadSection       => "Read".into(),
        Token::WriteSection      => "Write".into(),
        Token::CallSection       => "Call".into(),
        Token::ThrowSection      => "Throw".into(),
        Token::KernelAnnotation  => "[kernel]".into(),
        Token::Ident(s)          => s.clone(),
        Token::AccessModifier(s) => s.clone(),
        Token::RustKeyword(s)    => s.clone(),
        Token::LParen            => "(".into(),
        Token::RParen            => ")".into(),
        Token::LBrace            => "{".into(),
        Token::RBrace            => "}".into(),
        Token::Colon             => ":".into(),
        Token::Comma             => ",".into(),
        Token::Dot               => ".".into(),
        Token::BracketPair       => "[]".into(),
        Token::Other(c)          => c.to_string(),
    }
}

// ─── 契約ブロックパーサー ─────────────────────────────────────────────────────

fn parse_contract_block(block_text: &str, start_line: usize) -> Result<DeclaredContract, ParseError> {
    let inner = &block_text[1..block_text.len()-1];
    let mut line = start_line;
    let mut pos = 0usize;
    let mut contract = DeclaredContract::default();

    pos = skip_whitespace_and_comments(inner, pos, &mut line);

    while pos < inner.len() {
        pos = skip_whitespace_and_comments(inner, pos, &mut line);
        if pos >= inner.len() { break; }

        let section = if inner[pos..].starts_with("Read:") {
            pos += "Read:".len(); "Read"
        } else if inner[pos..].starts_with("Write:") {
            pos += "Write:".len(); "Write"
        } else if inner[pos..].starts_with("Call:") {
            pos += "Call:".len(); "Call"
        } else if inner[pos..].starts_with("Throw:") {
            pos += "Throw:".len(); "Throw"
        } else {
            return Err(ParseError {
                message: format!(
                    "契約ブロック内に未知のテキスト: {:?}",
                    inner[pos..].chars().take(20).collect::<String>()
                ),
                line: Some(line),
            });
        };

        // セクション内エントリーをコンマ区切りで読む
        loop {
            pos = skip_whitespace_and_comments(inner, pos, &mut line);
            if pos >= inner.len() { break; }
            if is_section_header(inner, pos) { break; }

            let (entry, after_entry) = read_contract_entry(inner, pos, &mut line)
                .ok_or_else(|| ParseError {
                    message: "契約エントリーの読み取りに失敗しました".into(),
                    line: Some(line),
                })?;
            pos = after_entry;

            match section {
                "Read" => {
                    // `as alias` チェック
                    let pa = skip_whitespace_and_comments(inner, pos, &mut line);
                    if inner[pa..].starts_with("as") {
                        let after_as_kw = pa + 2;
                        let next_c = inner[after_as_kw..].chars().next();
                        if next_c.map_or(false, |c| c.is_whitespace() || c == '_' || c.is_ascii_alphabetic()) {
                            let pa2 = skip_whitespace_and_comments(inner, after_as_kw, &mut line);
                            let (alias, after_alias) = read_ident(inner, pa2)
                                .ok_or_else(|| ParseError {
                                    message: "as の後ろにエイリアス名がありません".into(),
                                    line: Some(line),
                                })?;
                            contract.read.push(ReadPath { path: StructurePath(entry), alias: Some(alias) });
                            pos = after_alias;
                        } else {
                            contract.read.push(ReadPath { path: StructurePath(entry), alias: None });
                        }
                    } else {
                        contract.read.push(ReadPath { path: StructurePath(entry), alias: None });
                    }
                }
                "Write" => contract.write.push(StructurePath(entry)),
                "Call"  => contract.call.push(FunctionId(entry.trim_end_matches("()").to_string())),
                "Throw" => contract.throw.push(ThrowType(entry)),
                _ => {}
            }

            pos = skip_whitespace_and_comments(inner, pos, &mut line);
            if inner[pos..].starts_with(',') { pos += 1; } else { break; }
        }
    }

    Ok(contract)
}

fn is_section_header(input: &str, pos: usize) -> bool {
    let s = &input[pos..];
    s.starts_with("Read:") || s.starts_with("Write:") || s.starts_with("Call:") || s.starts_with("Throw:")
}

fn read_contract_entry(input: &str, pos: usize, _line: &mut usize) -> Option<(String, usize)> {
    let start = pos;
    let mut end = pos;
    let chars: Vec<char> = input[pos..].chars().collect();
    let mut i = 0usize;
    // :: プレフィックス（型レベルパス §6.7）
    if chars.get(0) == Some(&':') && chars.get(1) == Some(&':') {
        end += 2;
        i += 2;
    }
    while i < chars.len() {
        let c = chars[i];
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '.' | '(' | ')' => {
                end += c.len_utf8();
                i += 1;
            }
            '[' if chars.get(i+1) == Some(&']') => {
                end += 2;
                i += 2;
            }
            _ => break,
        }
    }
    if end == start { return None; }
    Some((input[start..end].to_string(), end))
}

// ─── ContractMethod パーサー ─────────────────────────────────────────────────

fn parse_contract_methods(body: &str, start_line: usize) -> Result<Vec<ContractMethod>, ParseError> {
    let mut methods = Vec::new();
    let mut pos = 0usize;
    let mut line = start_line;

    while pos < body.len() {
        pos = skip_whitespace_and_comments(body, pos, &mut line);
        if pos >= body.len() { break; }

        let (return_type, after_ret) = read_type_expr(body, pos, &mut line)
            .ok_or_else(|| ParseError { message: "contract メソッド戻り型読み取り失敗".into(), line: Some(line) })?;
        pos = after_ret;
        pos = skip_whitespace_and_comments(body, pos, &mut line);

        let (name, after_name) = read_ident(body, pos)
            .ok_or_else(|| ParseError { message: "contract メソッド名読み取り失敗".into(), line: Some(line) })?;
        pos = after_name;
        pos = skip_whitespace_and_comments(body, pos, &mut line);

        let (paren_text, after_params) = read_paren_block(body, pos, &mut line)
            .ok_or_else(|| ParseError { message: "contract メソッドパラメータ ')' が見つかりません".into(), line: Some(line) })?;
        let params = parse_params(&paren_text[1..paren_text.len()-1], line)?;
        pos = after_params;
        pos = skip_whitespace_and_comments(body, pos, &mut line);

        let contract = if body[pos..].starts_with('(') {
            let (cb_text, after_cb) = read_paren_block(body, pos, &mut line)
                .ok_or_else(|| ParseError { message: "contract メソッド契約ブロック ')' が見つかりません".into(), line: Some(line) })?;
            pos = after_cb;
            parse_contract_block(&cb_text, line)?
        } else {
            DeclaredContract::default()
        };

        pos = skip_whitespace_and_comments(body, pos, &mut line);
        if body[pos..].starts_with(';') { pos += 1; }

        methods.push(ContractMethod {
            id: FunctionId(name.clone()),
            sig: DdcFnSig { return_type, name: FunctionId(name), params },
            contract,
        });
    }
    Ok(methods)
}

// ─── ImplMethod パーサー ──────────────────────────────────────────────────────

fn parse_impl_methods(body: &str, start_line: usize) -> Result<Vec<ImplMethod>, ParseError> {
    let mut methods = Vec::new();
    let mut pos = 0usize;
    let mut line = start_line;

    while pos < body.len() {
        pos = skip_whitespace_and_comments(body, pos, &mut line);
        if pos >= body.len() { break; }

        // アクセス修飾子・readonly スキップ
        for kw in &["public", "private", "protected", "internal", "readonly"] {
            let p = skip_whitespace_and_comments(body, pos, &mut line);
            if body[p..].starts_with(kw) {
                let after = p + kw.len();
                if body[after..].starts_with(|c: char| c.is_whitespace()) {
                    pos = after;
                    pos = skip_whitespace_and_comments(body, pos, &mut line);
                }
            }
        }

        let (return_type, after_ret) = read_type_expr(body, pos, &mut line)
            .ok_or_else(|| ParseError { message: "class メソッド戻り型読み取り失敗".into(), line: Some(line) })?;
        pos = after_ret;
        pos = skip_whitespace_and_comments(body, pos, &mut line);

        let (name, after_name) = read_ident(body, pos)
            .ok_or_else(|| ParseError { message: "class メソッド名読み取り失敗".into(), line: Some(line) })?;
        pos = after_name;
        pos = skip_whitespace_and_comments(body, pos, &mut line);

        let (paren_text, after_params) = read_paren_block(body, pos, &mut line)
            .ok_or_else(|| ParseError { message: "class メソッドパラメータ ')' が見つかりません".into(), line: Some(line) })?;
        let params = parse_params(&paren_text[1..paren_text.len()-1], line)?;
        pos = after_params;
        pos = skip_whitespace_and_comments(body, pos, &mut line);

        // `: ContractId.MethodId` か `( Read: ... )` のどちらか（どちらもない場合はエラー）
        let kind = if body[pos..].starts_with(':') {
            pos += 1;
            pos = skip_whitespace_and_comments(body, pos, &mut line);

            let mut contract_refs = Vec::new();
            loop {
                let ref_start = pos;
                while pos < body.len() {
                    let c = body[pos..].chars().next().unwrap();
                    if c.is_ascii_alphanumeric() || c == '_' || c == '.' { pos += c.len_utf8(); } else { break; }
                }
                if pos == ref_start {
                    return Err(ParseError { message: "contract 参照名が空です".into(), line: Some(line) });
                }
                contract_refs.push(FunctionId(body[ref_start..pos].to_string()));
                pos = skip_whitespace_and_comments(body, pos, &mut line);
                if body[pos..].starts_with(',') { pos += 1; pos = skip_whitespace_and_comments(body, pos, &mut line); } else { break; }
            }
            ImplMethodKind::ContractImpl { contract_refs }
        } else if body[pos..].starts_with('(') {
            let (paren_text, after_paren) = read_paren_block(body, pos, &mut line)
                .ok_or_else(|| ParseError { message: "class 独自契約ブロック ')' が見つかりません".into(), line: Some(line) })?;
            pos = after_paren;
            let contract = parse_contract_block(&paren_text, line)?;
            ImplMethodKind::OwnContract { contract }
        } else {
            return Err(ParseError {
                message: "class メソッドに ':' か '( ... )' が必要です".into(),
                line: Some(line),
            });
        };

        pos = skip_whitespace_and_comments(body, pos, &mut line);

        let (body_text, after_body) = read_brace_block(body, pos, &mut line)
            .ok_or_else(|| ParseError { message: "class メソッド body '}' が見つかりません".into(), line: Some(line) })?;
        pos = after_body;

        methods.push(ImplMethod {
            id: FunctionId(name.clone()),
            sig: DdcFnSig { return_type, name: FunctionId(name), params },
            kind,
            body: DdcBody::Rust(body_text),
        });
    }
    Ok(methods)
}

// ─── パラメータパーサー ───────────────────────────────────────────────────────

fn parse_params(inner: &str, start_line: usize) -> Result<Vec<Param>, ParseError> {
    let mut params = Vec::new();
    let mut pos = 0usize;
    let mut line = start_line;

    loop {
        pos = skip_whitespace_and_comments(inner, pos, &mut line);
        if pos >= inner.len() { break; }

        let (type_name, after_type) = read_type_expr(inner, pos, &mut line)
            .ok_or_else(|| ParseError { message: "パラメータ型名読み取り失敗".into(), line: Some(line) })?;
        pos = after_type;
        pos = skip_whitespace_and_comments(inner, pos, &mut line);

        let (param_name, after_name) = read_ident(inner, pos)
            .ok_or_else(|| ParseError {
                message: format!("パラメータ名読み取り失敗（型: {}）", type_name),
                line: Some(line),
            })?;
        pos = after_name;

        params.push(Param { type_name, param_name });

        pos = skip_whitespace_and_comments(inner, pos, &mut line);
        if inner[pos..].starts_with(',') { pos += 1; } else { break; }
    }

    Ok(params)
}

// ─── テスト ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> DdcFile {
        parse_ddc_file(src).expect("parse failed")
    }

    // ── RustPassthrough ──────────────────────────────────────────────────────

    #[test]
    fn passthrough_struct() {
        let src = "struct Order { id: u32 }";
        let file = parse(src);
        assert_eq!(file.items.len(), 1);
        assert!(matches!(&file.items[0], DdcItem::RustPassthrough(s) if s.contains("struct Order")));
    }

    #[test]
    fn passthrough_use() {
        let src = "use std::collections::HashMap;";
        let file = parse(src);
        assert!(matches!(&file.items[0], DdcItem::RustPassthrough(_)));
    }

    // ── DdcFn 最小ケース ────────────────────────────────────────────────────

    #[test]
    fn minimal_fn() {
        let src = "void foo() () { }";
        let file = parse(src);
        assert_eq!(file.items.len(), 1);
        let DdcItem::DdcFn(f) = &file.items[0] else { panic!("not DdcFn") };
        assert_eq!(f.sig.return_type, "void");
        assert_eq!(f.id.0, "foo");
        assert!(f.sig.params.is_empty());
        assert!(f.contract.read.is_empty());
        assert!(!f.is_readonly);
        assert!(f.annotations.is_empty());
    }

    #[test]
    fn fn_no_contract_block() {
        // 契約ブロックなし → 空契約
        let src = "void bar() { }";
        let file = parse(src);
        let DdcItem::DdcFn(f) = &file.items[0] else { panic!() };
        assert_eq!(f.sig.name.0, "bar");
        assert!(f.contract.read.is_empty());
    }

    // ── 全セクション ─────────────────────────────────────────────────────────

    #[test]
    fn full_contract_sections() {
        let src = r#"
public void process_shipping(Order order)
(
    Read:
        order.id,
        order.items[].sku
    Write:
        order.status
    Throw:
        ShippingError
) {
    // body
}
"#;
        let file = parse(src);
        let DdcItem::DdcFn(f) = &file.items[0] else { panic!() };
        assert_eq!(f.contract.read.len(), 2);
        assert_eq!(f.contract.read[0].path.0, "order.id");
        assert_eq!(f.contract.read[1].path.0, "order.items[].sku");
        assert_eq!(f.contract.write[0].0, "order.status");
        assert_eq!(f.contract.throw[0].0, "ShippingError");
    }

    // ── エイリアス ───────────────────────────────────────────────────────────

    #[test]
    fn alias_in_read() {
        let src = r#"
void ship(Order order)
(
    Read:
        order.items[].sku as sku
) { }
"#;
        let file = parse(src);
        let DdcItem::DdcFn(f) = &file.items[0] else { panic!() };
        let rp = &f.contract.read[0];
        assert_eq!(rp.path.0, "order.items[].sku");
        assert_eq!(rp.alias, Some("sku".into()));
    }

    // ── readonly / [kernel] ──────────────────────────────────────────────────

    #[test]
    fn readonly_fn() {
        let src = r#"
public readonly Optional<User> find(UserId id)
(
    Read: id.value
    Call: repository.load()
) { }
"#;
        let file = parse(src);
        let DdcItem::DdcFn(f) = &file.items[0] else { panic!() };
        assert!(f.is_readonly);
        assert_eq!(f.sig.return_type, "Optional<User>");
        assert_eq!(f.contract.read[0].path.0, "id.value");
        assert_eq!(f.contract.call[0].0, "repository.load");
    }

    #[test]
    fn kernel_fn() {
        let src = r#"
[kernel]
public void send_bytes(Conn conn)
(
    Throw: IoError
) {
    unsafe { }
}
"#;
        let file = parse(src);
        let DdcItem::DdcFn(f) = &file.items[0] else { panic!() };
        assert!(f.annotations.contains(&DdcAnnotation::Kernel));
        assert_eq!(f.contract.throw[0].0, "IoError");
    }

    // ── contract / class ─────────────────────────────────────────────────────

    #[test]
    fn contract_decl() {
        let src = r#"
contract IRepository
(
    Read: id.value
)
{
    Optional<User> load(UserId id)
    (
        Read: id.value
    );
}
"#;
        let file = parse(src);
        let DdcItem::Contract(c) = &file.items[0] else { panic!() };
        assert_eq!(c.id.0, "IRepository");
        assert_eq!(c.envelope.read[0].path.0, "id.value");
        assert_eq!(c.methods.len(), 1);
        assert_eq!(c.methods[0].id.0, "load");
    }

    #[test]
    fn class_decl() {
        let src = r#"
class SqlRepository : IRepository {
    public Optional<User> load(UserId id) : IRepository.load {
        // body
    }
}
"#;
        let file = parse(src);
        let DdcItem::Impl(imp) = &file.items[0] else { panic!() };
        assert_eq!(imp.type_id.0, "SqlRepository");
        assert_eq!(imp.contract_ids[0].0, "IRepository");
        let ImplMethodKind::ContractImpl { contract_refs } = &imp.methods[0].kind else { panic!() };
        assert_eq!(contract_refs[0].0, "IRepository.load");
    }

    #[test]
    fn own_contract_method() {
        let src = r#"
class SqlRepository : IRepository {
    public Optional<User> load(UserId id) : IRepository.load {
        // body
    }
    private void helper(UserId id)
    (
        Read: id.value
        Call: db_query()
    ) {
        // helper body
    }
}
"#;
        let file = parse(src);
        let DdcItem::Impl(imp) = &file.items[0] else { panic!() };
        assert_eq!(imp.methods.len(), 2);
        // 1つ目: ContractImpl
        let ImplMethodKind::ContractImpl { contract_refs } = &imp.methods[0].kind else { panic!() };
        assert_eq!(contract_refs[0].0, "IRepository.load");
        // 2つ目: OwnContract
        let ImplMethodKind::OwnContract { contract } = &imp.methods[1].kind else { panic!() };
        assert_eq!(contract.read[0].path.0, "id.value");
        assert_eq!(contract.call[0].0, "db_query");
    }

    #[test]
    fn multi_contract_class() {
        let src = r#"
class FileCache : IReader, IWriter {
    public Data read(FileId id) : IReader.read {
        // body
    }
    public void write(FileId id, Data d) : IWriter.write {
        // body
    }
}
"#;
        let file = parse(src);
        let DdcItem::Impl(imp) = &file.items[0] else { panic!() };
        assert_eq!(imp.type_id.0, "FileCache");
        assert_eq!(imp.contract_ids.len(), 2);
        assert_eq!(imp.contract_ids[0].0, "IReader");
        assert_eq!(imp.contract_ids[1].0, "IWriter");
        assert_eq!(imp.methods.len(), 2);
    }

    #[test]
    fn multi_contract_ref() {
        let src = r#"
class DualReader : IReader, ICache {
    public Data read(FileId id) : IReader.read, ICache.read {
        // body
    }
}
"#;
        let file = parse(src);
        let DdcItem::Impl(imp) = &file.items[0] else { panic!() };
        assert_eq!(imp.contract_ids.len(), 2);
        let ImplMethodKind::ContractImpl { contract_refs } = &imp.methods[0].kind else { panic!() };
        assert_eq!(contract_refs.len(), 2);
        assert_eq!(contract_refs[0].0, "IReader.read");
        assert_eq!(contract_refs[1].0, "ICache.read");
    }

    // ── 複数アイテム ─────────────────────────────────────────────────────────

    #[test]
    fn multiple_items() {
        let src = r#"
struct Foo { x: u32 }
void bar() { }
"#;
        let file = parse(src);
        assert_eq!(file.items.len(), 2);
        assert!(matches!(&file.items[0], DdcItem::RustPassthrough(_)));
        assert!(matches!(&file.items[1], DdcItem::DdcFn(_)));
    }

    // ── コンマ区切りの複数エントリー ─────────────────────────────────────────

    #[test]
    fn multi_entry_comma() {
        let src = r#"
void transfer(Account from, Account to)
(
    Read: from.balance, to.id
    Write: from.balance, to.balance
    Throw: InsufficientFundsError, AccountLockedError
) { }
"#;
        let file = parse(src);
        let DdcItem::DdcFn(f) = &file.items[0] else { panic!() };
        assert_eq!(f.contract.read.len(), 2);
        assert_eq!(f.contract.write.len(), 2);
        assert_eq!(f.contract.throw.len(), 2);
    }

    // ── 複数アイテム連続（回帰テスト）───────────────────────────────────────
    // 修正前: parse_ddc_fn_sig / parse_contract_decl / parse_impl_decl の末尾で
    // align_token_pos_to_raw が \n を指すとトークンカーソルが tokens.len() まで
    // 走り去り、後続アイテムの readonly / contract / class が認識されなかった。

    #[test]
    fn two_fns_in_sequence() {
        // DDC 関数が 2 つ連続するケース
        let src = r#"
void foo() { }
void bar() { }
"#;
        let file = parse(src);
        assert_eq!(file.items.len(), 2);
        assert!(matches!(&file.items[0], DdcItem::DdcFn(_)));
        assert!(matches!(&file.items[1], DdcItem::DdcFn(_)));
    }

    #[test]
    fn readonly_fn_after_fn() {
        // readonly 関数の直前に別の DDC 関数がある場合
        // （修正前: 2 番目の readonly が認識されず戻り型として読まれた）
        let src = r#"
Timer new_timer() { Timer { elapsed_ms: 0 } }
readonly u64 elapsed(Timer t) (Read: t.elapsed_ms) { t.elapsed_ms }
"#;
        let file = parse(src);
        assert_eq!(file.items.len(), 2);
        let DdcItem::DdcFn(f) = &file.items[1] else { panic!() };
        assert!(f.is_readonly);
        assert_eq!(f.sig.return_type, "u64");
        assert_eq!(f.sig.name.0, "elapsed");
    }

    #[test]
    fn fn_with_contract_then_fn() {
        // 契約ブロック付き関数の後に別の関数が続くケース
        let src = r#"
readonly bool check(T t) (Read: t.value) { t.value }
void apply(T t) { }
"#;
        let file = parse(src);
        assert_eq!(file.items.len(), 2);
        let DdcItem::DdcFn(second) = &file.items[1] else { panic!() };
        assert_eq!(second.sig.name.0, "apply");
    }

    #[test]
    fn contract_then_class() {
        // contract の後に class が続くケース
        // （修正前: contract の align_token_pos_to_raw がカーソルを末尾まで進め
        //   後続の class が DdcFn として誤パースされた）
        let src = r#"
contract IFoo {
    void run(T t);
}
class FooImpl : IFoo {
    void run(T t) : IFoo.run { }
}
"#;
        let file = parse(src);
        assert_eq!(file.items.len(), 2);
        assert!(matches!(&file.items[0], DdcItem::Contract(_)));
        assert!(matches!(&file.items[1], DdcItem::Impl(_)));
    }

    #[test]
    fn fn_contract_class_sequence() {
        // DDC 関数 → contract → class の 3 アイテム連続
        let src = r#"
void foo() { }
contract IFoo { void run(T t); }
class FooImpl : IFoo { void run(T t) : IFoo.run { } }
"#;
        let file = parse(src);
        assert_eq!(file.items.len(), 3);
        assert!(matches!(&file.items[0], DdcItem::DdcFn(_)));
        assert!(matches!(&file.items[1], DdcItem::Contract(_)));
        assert!(matches!(&file.items[2], DdcItem::Impl(_)));
    }

    #[test]
    fn multiple_readonly_fns() {
        // readonly 関数が複数連続するケース
        let src = r#"
readonly u64 get_a(T t) (Read: t.a) { t.a }
readonly u64 get_b(T t) (Read: t.b) { t.b }
readonly u64 get_c(T t) (Read: t.c) { t.c }
"#;
        let file = parse(src);
        assert_eq!(file.items.len(), 3);
        for item in &file.items {
            let DdcItem::DdcFn(f) = item else { panic!() };
            assert!(f.is_readonly);
        }
    }

    // ── フェーズ 6: 型レベル Write パス / has_contract_block ─────────────────

    #[test]
    fn type_level_write_path() {
        // Write: ::TypeName.field パースの確認
        let src = r#"
Timer new_timer()
(Write: ::Timer.elapsed_ms)
{ Timer { elapsed_ms: 0 } }
"#;
        let file = parse(src);
        let DdcItem::DdcFn(f) = &file.items[0] else { panic!() };
        assert_eq!(f.contract.write.len(), 1);
        assert_eq!(f.contract.write[0].0, "::Timer.elapsed_ms");
        assert!(f.has_contract_block);
    }

    #[test]
    fn type_level_write_multiple() {
        // 複数の型レベルパスを含む Write: セクション
        let src = r#"
Alarm new_alarm(String label, u64 threshold_ms)
(Write: ::Alarm.label, ::Alarm.threshold_ms, ::Alarm.fired)
{ Alarm { label, threshold_ms, fired: false } }
"#;
        let file = parse(src);
        let DdcItem::DdcFn(f) = &file.items[0] else { panic!() };
        assert_eq!(f.contract.write.len(), 3);
        assert_eq!(f.contract.write[0].0, "::Alarm.label");
        assert_eq!(f.contract.write[1].0, "::Alarm.threshold_ms");
        assert_eq!(f.contract.write[2].0, "::Alarm.fired");
    }

    #[test]
    fn has_contract_block_true() {
        let src = "void foo() (Read: x.y) { }";
        let file = parse(src);
        let DdcItem::DdcFn(f) = &file.items[0] else { panic!() };
        assert!(f.has_contract_block);
    }

    #[test]
    fn has_contract_block_false() {
        // 契約ブロックなし → has_contract_block == false
        let src = "void bar() { }";
        let file = parse(src);
        let DdcItem::DdcFn(f) = &file.items[0] else { panic!() };
        assert!(!f.has_contract_block);
    }

    #[test]
    fn mixed_write_paths() {
        // 変数レベルと型レベルの混在
        let src = r#"
Timer tick(Timer timer)
(
    Read: timer.elapsed_ms,
    Write: ::Timer.elapsed_ms
)
{ Timer { elapsed_ms: timer.elapsed_ms + 1, ..timer } }
"#;
        let file = parse(src);
        let DdcItem::DdcFn(f) = &file.items[0] else { panic!() };
        assert_eq!(f.contract.read.len(), 1);
        assert_eq!(f.contract.write.len(), 1);
        assert_eq!(f.contract.write[0].0, "::Timer.elapsed_ms");
        assert!(f.has_contract_block);
    }
}
