// BodyAnalyzer: syn による body DDC 適合性検証（Tech-onRust.md §11.14 参照）

use std::collections::HashSet;
use syn::visit::Visit;
use ddc_syntax::{DdcAnnotation, DdcBody, DdcFn, FunctionId};
use ddc_core::{ValidationError, ValidationErrorKind};

/// DDC 関数の body が宣言契約と適合しているかを検証する
pub(crate) struct BodyAnalyzer;

impl BodyAnalyzer {
    /// body 内の実際のアクセスが宣言契約と一致することを検証する。
    /// 複数のエラーをまとめて返す。
    pub(crate) fn validate(func: &DdcFn) -> Vec<ValidationError> {
        let DdcBody::Rust(body_code) = &func.body;

        let block = match syn::parse_str::<syn::Block>(body_code) {
            Ok(b) => b,
            Err(_) => return vec![],  // 構文エラーは後段の rustc に委ねる
        };

        let is_kernel = func.annotations.iter().any(|a| *a == DdcAnnotation::Kernel);

        // alias → canonical path の名前セット（alias 呼び出しは Call: 未宣言扱いにしない）
        let alias_names: HashSet<String> = func.contract.read.iter()
            .filter_map(|rp| rp.alias.clone())
            .collect();

        // Read: + Write:(変数レベルのみ) の正規化パスセット（read check は両方 OK）
        let declared_reads: HashSet<String> = func.contract.read.iter()
            .map(|rp| normalize_path(&rp.path.0))
            .chain(func.contract.write.iter()
                .filter(|sp| !sp.0.starts_with("::"))  // 型レベルパスは除外
                .map(|sp| normalize_path(&sp.0)))
            .collect();

        // Write: 変数レベルパスセット（`::` なし）
        let declared_var_writes: HashSet<String> = func.contract.write.iter()
            .filter(|sp| !sp.0.starts_with("::"))
            .map(|sp| normalize_path(&sp.0))
            .collect();

        // Write: 型レベルパスセット（`::TypeName.field` → `(TypeName, field)` ペア）
        let declared_type_writes: HashSet<(String, String)> = func.contract.write.iter()
            .filter(|sp| sp.0.starts_with("::"))
            .filter_map(|sp| parse_type_level_path(&sp.0))
            .collect();

        // Call: のセット
        let declared_calls: HashSet<String> = func.contract.call.iter()
            .map(|fid| fid.0.clone())
            .collect();

        let mut visitor = BodyVisitor {
            func_id: &func.id,
            is_kernel,
            return_type_root: root_type_ident(&func.sig.return_type),
            declared_reads,
            declared_var_writes,
            declared_type_writes,
            declared_calls,
            alias_names,
            errors: Vec::new(),
        };

        visitor.visit_block(&block);
        visitor.errors
    }
}

// ─── 型レベルパスパーサ ────────────────────────────────────────────────

/// `::TypeName.field` 形式の型レベルパスを `(TypeName, field)` ペアに分解する。
fn parse_type_level_path(path: &str) -> Option<(String, String)> {
    let rest = path.strip_prefix("::")? ;
    let dot = rest.find('.')?;
    Some((rest[..dot].to_string(), rest[dot+1..].to_string()))
}

// ─── パス正規化 ──────────────────────────────────────────────────────────────

/// DDC パスの `[]` を除去し正規化する。"order.items[].sku" → "order.items.sku"
fn normalize_path(path: &str) -> String {
    path.replace("[]", "")
        .split('.')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

fn root_type_ident(type_name: &str) -> Option<String> {
    let trimmed = type_name.trim();
    if trimmed.is_empty() || trimmed == "void" {
        return None;
    }
    let root = trimmed
        .split(['<', ':', '.', ' ', ','])
        .next()
        .unwrap_or_default()
        .trim();
    if root.is_empty() { None } else { Some(root.to_string()) }
}

/// syn の Expr からドット区切りフィールドパスを再構築する。
/// 単純な `a.b.c` や `a.b[i].c`、`&a.b` の形式のみ対応。複合式は None を返す。
fn collect_field_path(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Field(f) => collect_field_path_from_field(f),
        syn::Expr::Index(i) => collect_field_path(&i.expr),  // a.b[i] → a.b
        syn::Expr::Path(p) if p.path.segments.len() == 1 => {
            Some(p.path.segments[0].ident.to_string())
        }
        syn::Expr::Reference(r) => collect_field_path(&r.expr),  // &a.b → a.b
        _ => None,
    }
}

fn collect_field_path_from_field(node: &syn::ExprField) -> Option<String> {
    let member = match &node.member {
        syn::Member::Named(ident) => ident.to_string(),
        syn::Member::Unnamed(_) => return None,
    };
    let base = collect_field_path(&node.base)?;
    Some(format!("{}.{}", base, member))
}

/// 宣言セットに対してパスが包含されているかを確認する。
/// "order.items" が宣言されていれば "order.items.sku" も包含される（接頭辞照合）。
fn is_covered(path: &str, declared: &HashSet<String>) -> bool {
    declared.contains(path)
        || declared.iter().any(|decl| path.starts_with(&format!("{}.", decl)))
}

// ─── BodyVisitor ─────────────────────────────────────────────────────────────

struct BodyVisitor<'a> {
    func_id:              &'a FunctionId,
    is_kernel:            bool,
    return_type_root:     Option<String>,
    declared_reads:       HashSet<String>,
    declared_var_writes:  HashSet<String>,
    declared_type_writes: HashSet<(String, String)>,
    declared_calls:       HashSet<String>,
    alias_names:          HashSet<String>,
    errors:               Vec<ValidationError>,
}

impl<'a> BodyVisitor<'a> {
    fn push_error(&mut self, kind: ValidationErrorKind, msg: String) {
        self.errors.push(ValidationError {
            kind,
            function: self.func_id.clone(),
            message:  msg,
        });
    }
}

impl<'a, 'ast> Visit<'ast> for BodyVisitor<'a> {
    // ── フィールド読み取り ────────────────────────────────────────────────────
    fn visit_expr_field(&mut self, node: &'ast syn::ExprField) {
        if let Some(path) = collect_field_path_from_field(node) {
            // トップレベルの完全パスを read としてチェック（サブパスへの再帰はしない）
            if !is_covered(&path, &self.declared_reads) {
                self.push_error(
                    ValidationErrorKind::BodyUndeclaredRead,
                    format!("未宣言パスへの読み取り: '{}'", path),
                );
            }
        } else {
            // 複合式のベース → デフォルト再帰で内部を検査
            syn::visit::visit_expr_field(self, node);
        }
    }

    // ── フィールド書き込み（代入 LHS）────────────────────────────────────────
    fn visit_expr_assign(&mut self, node: &'ast syn::ExprAssign) {
        // LHS: write target としてチェック（変数レベルパスのみ）
        if let Some(path) = collect_field_path(&node.left) {
            if !is_covered(&path, &self.declared_var_writes) {
                self.push_error(
                    ValidationErrorKind::BodyUndeclaredWrite,
                    format!("未宣言パスへの書き込み: '{}'", path),
                );
            }
        }
        // RHS: 通常の read として再帰
        syn::visit::visit_expr(self, &node.right);
    }

    // ── 構造体リテラル（型レベル Write）──────────────────────────────────
    fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
        // 型名取得（一番右のセグメント = 最内ノ型名）
        let type_name = node.path.segments.last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();

        // 明示的に初期化されたフィールドのみを検査（`..base` spread は除外）
        for field in &node.fields {
            if let syn::Member::Named(ident) = &field.member {
                let field_name = ident.to_string();
                let key = (type_name.clone(), field_name.clone());
                if !self.declared_type_writes.contains(&key) {
                    self.push_error(
                        ValidationErrorKind::BodyUndeclaredWrite,
                        format!("未宣言の型レベル Write パス: '::{}.{}'", type_name, field_name),
                    );
                }
            }
        }
        // フィールド値式と ..base を read として再帰検査
        syn::visit::visit_expr_struct(self, node);
    }

    // ── 直接関数呼び出し ─────────────────────────────────────────────────────
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = node.func.as_ref() {
            let segments: Vec<String> = p.path.segments.iter()
                .map(|s| s.ident.to_string())
                .collect();
            if !segments.is_empty() {
                let dotted = segments.join(".");

                // コンストラクタ相当呼び出し:
                // - Type::new(...)        => Type.Constructor
                // - Type(...) (tuple等)   => Type.Constructor
                let constructor_candidate = if segments.last().map(|s| s == "new").unwrap_or(false) && segments.len() >= 2 {
                    let owner_leaf = &segments[segments.len() - 2];
                    if self.return_type_root.as_ref().map(|rt| rt == owner_leaf).unwrap_or(false) {
                        Some(format!("{}.Constructor", segments[..segments.len() - 1].join(".")))
                    } else {
                        None
                    }
                } else {
                    None
                };

                let is_declared = if segments.len() == 1 {
                    // 既存仕様: 単純呼び出しのみ Call: を照合
                    self.alias_names.contains(&dotted) || self.declared_calls.contains(&dotted)
                } else if let Some(constructor) = constructor_candidate.as_ref() {
                    // 追加仕様: 戻り値型を構築する Type::new(...) は *.Constructor 宣言を要求
                    self.declared_calls.contains(constructor)
                        || self.declared_calls.contains(&format!("::{}", constructor))
                } else {
                    // issue 対応の最小変更として、従来未検証だった多段パス呼び出しは現状維持
                    true
                };

                if !is_declared {
                    let shown = constructor_candidate.unwrap_or(dotted);
                    self.push_error(
                        ValidationErrorKind::BodyUndeclaredCall,
                        format!("未宣言関数の呼び出し: '{}'", shown),
                    );
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    // ── unsafe ブロック ──────────────────────────────────────────────────────
    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        if !self.is_kernel {
            self.push_error(
                ValidationErrorKind::BodyUnsafeBlock,
                "非 [kernel] 関数の body に unsafe ブロックが存在します".to_string(),
            );
        }
        syn::visit::visit_expr_unsafe(self, node);
    }

    // ── 禁止マクロ（panic!）：式中 ─────────────────────────────────────────
    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        if let Some(seg) = node.mac.path.segments.last() {
            if seg.ident == "panic" {
                self.push_error(
                    ValidationErrorKind::BodyForbiddenMacro,
                    "禁止マクロ: panic!".to_string(),
                );
            }
        }
        syn::visit::visit_expr_macro(self, node);
    }

    // ── 禁止マクロ（panic!）：文中（セミコロン付き `panic!(...);`）───────
    fn visit_stmt_macro(&mut self, node: &'ast syn::StmtMacro) {
        if let Some(seg) = node.mac.path.segments.last() {
            if seg.ident == "panic" {
                self.push_error(
                    ValidationErrorKind::BodyForbiddenMacro,
                    "禁止マクロ: panic!".to_string(),
                );
            }
        }
        syn::visit::visit_stmt_macro(self, node);
    }

    // ── 禁止メソッド（unwrap/expect）────────────────────────────────────────
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let method = node.method.to_string();
        if method == "unwrap" || method == "expect" {
            self.push_error(
                ValidationErrorKind::BodyForbiddenMacro,
                format!("禁止メソッド: .{}()", method),
            );
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ddc_syntax::{
        DdcAnnotation, DdcBody, DdcFn, DdcFnSig, DeclaredContract,
        FunctionId, ReadPath, StructurePath,
    };

    fn make_fn(
        name: &str,
        contract: DeclaredContract,
        body: &str,
        kernel: bool,
    ) -> DdcFn {
        DdcFn {
            annotations: if kernel { vec![DdcAnnotation::Kernel] } else { vec![] },
            id:          FunctionId(name.to_string()),
            is_readonly: false,
            has_contract_block: true,
            sig:         DdcFnSig {
                return_type: "void".to_string(),
                name:        FunctionId(name.to_string()),
                params:      vec![],
            },
            contract,
            body:        DdcBody::Rust(body.to_string()),
        }
    }

    // ── BodyUndeclaredRead ────────────────────────────────────────────────────

    #[test]
    fn undeclared_read_error() {
        // 宣言: Read: order.amount  /  アクセス: order.status → BodyUndeclaredRead
        let f = make_fn(
            "f",
            DeclaredContract {
                read: vec![ReadPath { path: StructurePath("order.amount".into()), alias: None }],
                ..Default::default()
            },
            "{ let _ = order.status; }",
            false,
        );
        let errors = BodyAnalyzer::validate(&f);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::BodyUndeclaredRead);
    }

    // ── BodyUndeclaredWrite ───────────────────────────────────────────────────

    #[test]
    fn undeclared_write_error() {
        // 宣言: Write: order.amount  /  代入: order.status = 0 → BodyUndeclaredWrite
        let f = make_fn(
            "f",
            DeclaredContract {
                write: vec![StructurePath("order.amount".into())],
                ..Default::default()
            },
            "{ order.status = 0; }",
            false,
        );
        let errors = BodyAnalyzer::validate(&f);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::BodyUndeclaredWrite);
    }

    // ── BodyUndeclaredCall ────────────────────────────────────────────────────

    #[test]
    fn undeclared_call_error() {
        // 宣言: Call: good_func  /  呼び出し: bad_func() → BodyUndeclaredCall
        let f = make_fn(
            "f",
            DeclaredContract {
                call: vec![FunctionId("good_func".into())],
                ..Default::default()
            },
            "{ bad_func(); }",
            false,
        );
        let errors = BodyAnalyzer::validate(&f);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::BodyUndeclaredCall);
    }

    #[test]
    fn constructor_call_requires_declared_constructor() {
        // 宣言なし: Query::new() は Query.Constructor の未宣言呼び出しとして扱う
        let mut f = make_fn(
            "f",
            DeclaredContract::default(),
            "{ let _q = Query::new(connection); }",
            false,
        );
        f.sig.return_type = "Query".into();
        let errors = BodyAnalyzer::validate(&f);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::BodyUndeclaredCall);
        assert!(errors[0].message.contains("Query.Constructor"));
    }

    #[test]
    fn constructor_call_ok_when_declared() {
        // 宣言あり: Query::new() に対して Query.Constructor を許可
        let mut f = make_fn(
            "f",
            DeclaredContract {
                call: vec![FunctionId("Query.Constructor".into())],
                ..Default::default()
            },
            "{ let _q = Query::new(connection); }",
            false,
        );
        f.sig.return_type = "Query".into();
        let errors = BodyAnalyzer::validate(&f);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    // ── BodyUnsafeBlock ───────────────────────────────────────────────────────

    #[test]
    fn kernel_unsafe_ok() {
        // [kernel] 関数の unsafe ブロックはエラーなし
        let f = make_fn(
            "k",
            DeclaredContract::default(),
            "{ unsafe { let _x = 1; } }",
            true,
        );
        let errors = BodyAnalyzer::validate(&f);
        assert!(errors.is_empty());
    }

    #[test]
    fn non_kernel_unsafe_error() {
        // 非 [kernel] 関数の unsafe ブロック → BodyUnsafeBlock
        let f = make_fn(
            "f",
            DeclaredContract::default(),
            "{ unsafe { let _x = 1; } }",
            false,
        );
        let errors = BodyAnalyzer::validate(&f);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::BodyUnsafeBlock);
    }

    // ── BodyForbiddenMacro ────────────────────────────────────────────────────

    #[test]
    fn panic_macro_error() {
        // panic! → BodyForbiddenMacro
        let f = make_fn(
            "f",
            DeclaredContract::default(),
            r#"{ panic!("unreachable"); }"#,
            false,
        );
        let errors = BodyAnalyzer::validate(&f);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::BodyForbiddenMacro);
    }

    // ── エイリアス経由アクセス ────────────────────────────────────────────────

    #[test]
    fn alias_access_ok() {
        // Read: order.customer.city as city  /  body: city() → エラーなし
        // city() は codegen 生成クロージャへの呼び出しであり、宣言済みアクセス
        let f = make_fn(
            "f",
            DeclaredContract {
                read: vec![ReadPath {
                    path:  StructurePath("order.customer.city".into()),
                    alias: Some("city".into()),
                }],
                ..Default::default()
            },
            "{ let _ = city(); }",
            false,
        );
        let errors = BodyAnalyzer::validate(&f);
        assert!(errors.is_empty());
    }

    // ── visit_expr_struct: 型レベル Write パス ───────────────────────────────

    #[test]
    fn struct_literal_type_level_write_ok() {
        // Write: ::Timer.elapsed_ms 宣言あり → struct リテラルの elapsed_ms 初期化 OK
        let f = make_fn(
            "new_timer",
            DeclaredContract {
                write: vec![StructurePath("::Timer.elapsed_ms".into())],
                ..Default::default()
            },
            "{ Timer { elapsed_ms: 0 } }",
            false,
        );
        let errors = BodyAnalyzer::validate(&f);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn struct_literal_type_level_write_undeclared() {
        // Write: 宣言なし → struct リテラルの elapsed_ms 初期化 → BodyUndeclaredWrite
        let f = make_fn(
            "new_timer",
            DeclaredContract::default(),
            "{ Timer { elapsed_ms: 0 } }",
            false,
        );
        let errors = BodyAnalyzer::validate(&f);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::BodyUndeclaredWrite);
    }

    #[test]
    fn struct_literal_multiple_fields_ok() {
        // Write: ::Alarm.label, ::Alarm.threshold_ms, ::Alarm.fired → 全フィールド OK
        let f = make_fn(
            "new_alarm",
            DeclaredContract {
                write: vec![
                    StructurePath("::Alarm.label".into()),
                    StructurePath("::Alarm.threshold_ms".into()),
                    StructurePath("::Alarm.fired".into()),
                ],
                ..Default::default()
            },
            "{ Alarm { label: String::new(), threshold_ms: 0, fired: false } }",
            false,
        );
        let errors = BodyAnalyzer::validate(&f);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn struct_literal_partial_declaration_error() {
        // Write: ::Alarm.fired のみ → label, threshold_ms が未宣言 → 2エラー
        let f = make_fn(
            "new_alarm",
            DeclaredContract {
                write: vec![StructurePath("::Alarm.fired".into())],
                ..Default::default()
            },
            "{ Alarm { label: String::new(), threshold_ms: 0, fired: false } }",
            false,
        );
        let errors = BodyAnalyzer::validate(&f);
        assert_eq!(errors.len(), 2, "expected 2 errors for label + threshold_ms, got: {:?}", errors);
        assert!(errors.iter().all(|e| e.kind == ValidationErrorKind::BodyUndeclaredWrite));
    }
}
