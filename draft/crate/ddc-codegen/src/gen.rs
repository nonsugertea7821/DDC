// コード生成メイン（Tech-onRust.md §7 参照）

use std::collections::HashMap;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use ddc_syntax::{
    ContractDecl, DdcBody, DdcFile, DdcFn, DdcItem, ImplDecl, ImplMethod,
    ImplMethodKind, ReadPath,
};
use crate::{CodegenError, sig::convert_type};

/// DdcFile AST から Rust TokenStream を生成する（Tech-onRust.md §7）
pub fn codegen(file: &DdcFile) -> Result<TokenStream, CodegenError> {
    let mut out = TokenStream::new();

    // 型エイリアス preamble（C# スタイル型名を body 内で使えるようにする）
    out.extend(quote! {
        pub type Optional<T> = Option<T>;
        pub type List<T> = Vec<T>;
    });

    for item in &file.items {
        let ts = match item {
            DdcItem::RustPassthrough(text) => text
                .parse::<TokenStream>()
                .map_err(|e| CodegenError { message: format!("passthrough parse: {e}") })?,
            DdcItem::DdcFn(f)       => gen_fn(f)?,
            DdcItem::Contract(c)    => gen_contract(c)?,
            DdcItem::Impl(i)        => gen_impl(i)?,
        };
        out.extend(ts);
    }
    Ok(out)
}

// ─── DdcFn → fn ──────────────────────────────────────────────────────────────

fn gen_fn(func: &DdcFn) -> Result<TokenStream, CodegenError> {
    let name = format_ident!("{}", func.sig.name.0);
    let params = gen_params(&func.sig.params);
    let ret = type_ts(&func.sig.return_type)?;
    let body = gen_body_with_aliases(&func.body, &func.contract.read)?;

    Ok(quote! {
        fn #name(#params) -> #ret #body
    })
}

// ─── Contract → trait ────────────────────────────────────────────────────────

fn gen_contract(decl: &ContractDecl) -> Result<TokenStream, CodegenError> {
    let trait_name = format_ident!("{}", decl.id.0);
    let mut methods = TokenStream::new();
    for m in &decl.methods {
        let method_name = format_ident!("{}", m.id.0);
        let params = gen_params_with_self(&m.sig.params);
        let ret = type_ts(&m.sig.return_type)?;
        methods.extend(quote! {
            fn #method_name(#params) -> #ret;
        });
    }
    Ok(quote! {
        trait #trait_name {
            #methods
        }
    })
}

// ─── Impl → impl ─────────────────────────────────────────────────────────────

fn gen_impl(decl: &ImplDecl) -> Result<TokenStream, CodegenError> {
    let type_name = format_ident!("{}", decl.type_id.0);
    let mut out = TokenStream::new();

    // OwnContract メソッド → impl TypeId { ... }
    let own: Vec<_> = decl.methods.iter()
        .filter(|m| matches!(m.kind, ImplMethodKind::OwnContract { .. }))
        .collect();
    if !own.is_empty() {
        let m_ts = gen_impl_methods(&own)?;
        out.extend(quote! {
            impl #type_name {
                #m_ts
            }
        });
    }

    // ContractImpl メソッドをコントラクト ID 別にグループ化
    let mut by_contract: HashMap<String, Vec<&ImplMethod>> = HashMap::new();
    for m in &decl.methods {
        if let ImplMethodKind::ContractImpl { contract_refs } = &m.kind {
            if let Some(fid) = contract_refs.first() {
                let cid = contract_id_prefix(&fid.0).to_string();
                by_contract.entry(cid).or_default().push(m);
            }
        }
    }

    // 宣言順に emit（contract_ids の順序を保持）
    for cid in &decl.contract_ids {
        let contract_ident = format_ident!("{}", cid.0);
        let methods = by_contract.get(&cid.0).map(|v| v.as_slice()).unwrap_or(&[]);
        let m_ts = gen_impl_methods(methods)?;
        out.extend(quote! {
            impl #contract_ident for #type_name {
                #m_ts
            }
        });
    }

    Ok(out)
}

fn gen_impl_methods(methods: &[&ImplMethod]) -> Result<TokenStream, CodegenError> {
    let mut out = TokenStream::new();
    for m in methods {
        let name = format_ident!("{}", m.id.0);
        let params = gen_params_with_self(&m.sig.params);
        let ret = type_ts(&m.sig.return_type)?;
        // OwnContract は alias クロージャを挿入、ContractImpl は body のみ emit
        let reads: &[ReadPath] = match &m.kind {
            ImplMethodKind::OwnContract { contract } => &contract.read,
            ImplMethodKind::ContractImpl { .. }      => &[],
        };
        let body = gen_body_with_aliases(&m.body, reads)?;
        out.extend(quote! {
            fn #name(#params) -> #ret #body
        });
    }
    Ok(out)
}

// ─── 補助: パラメータ生成 ─────────────────────────────────────────────────────

/// DDC params を Rust スタイル `name: Type, ...` に変換（self なし）
fn gen_params(params: &[ddc_syntax::Param]) -> TokenStream {
    let mut out = TokenStream::new();
    for p in params {
        let name: TokenStream = p.param_name.parse().unwrap_or_default();
        let ty: TokenStream = convert_type(&p.type_name).parse().unwrap_or_default();
        out.extend(quote! { #name: #ty, });
    }
    out
}

/// trait / impl メソッド用: `&self` を先頭に付加
fn gen_params_with_self(params: &[ddc_syntax::Param]) -> TokenStream {
    let rest = gen_params(params);
    quote! { &self, #rest }
}

// ─── 補助: 型名変換 ──────────────────────────────────────────────────────────

fn type_ts(ddc_type: &str) -> Result<TokenStream, CodegenError> {
    convert_type(ddc_type)
        .parse::<TokenStream>()
        .map_err(|e| CodegenError { message: format!("type parse '{ddc_type}': {e}") })
}

// ─── 補助: body + alias クロージャ ───────────────────────────────────────────

/// `Read:` の alias エントリからクロージャ文文字列を生成する
fn gen_alias_closures_str(reads: &[ReadPath]) -> String {
    reads.iter()
        .filter_map(|rp| {
            let alias = rp.alias.as_deref()?;
            let path = &rp.path.0;
            let stmt = if let Some(idx) = path.find("[].") {
                // order.items[].sku → let sku = || order.items.iter().map(|__n| &__n.sku);
                let coll  = &path[..idx];
                let field = &path[idx + 3..];
                format!("let {alias} = || {coll}.iter().map(|__n| &__n.{field});")
            } else if path.ends_with("[]") {
                // order.items[] → let items = || order.items.iter();
                let base = &path[..path.len() - 2];
                format!("let {alias} = || {base}.iter();")
            } else {
                // order.customer.city → let city = || &order.customer.city;
                format!("let {alias} = || &{path};")
            };
            Some(stmt)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// body 文字列の `{ ... }` の中に alias クロージャを先頭挿入した TokenStream を返す
fn gen_body_with_aliases(body: &DdcBody, reads: &[ReadPath]) -> Result<TokenStream, CodegenError> {
    let DdcBody::Rust(body_str) = body;
    let alias_code = gen_alias_closures_str(reads);

    let combined = if alias_code.is_empty() {
        body_str.clone()
    } else {
        // "{ ...original... }" → "{ <aliases>\n...original... }"
        let trimmed = body_str.trim();
        let inner = trimmed
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
            .unwrap_or(trimmed);
        format!("{{\n{alias_code}\n{inner}\n}}")
    };

    combined.parse::<TokenStream>()
        .map_err(|e| CodegenError { message: format!("body tokenize: {e}") })
}

// ─── 補助: contract 参照の型部分を取得 ───────────────────────────────────────

/// "IOrderService.process" → "IOrderService"
fn contract_id_prefix(qualified: &str) -> &str {
    qualified.split('.').next().unwrap_or(qualified)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ddc_syntax::{
        DdcBody, DdcFn, DdcFnSig, DeclaredContract,
        DdcFile, DdcItem, FunctionId, Param, ReadPath, StructurePath,
        TypeId, ContractDecl, ContractMethod, ImplDecl, ImplMethod, ImplMethodKind,
    };

    fn param(type_name: &str, param_name: &str) -> Param {
        Param { type_name: type_name.into(), param_name: param_name.into() }
    }

    fn fn_sig(name: &str, params: Vec<Param>, ret: &str) -> DdcFnSig {
        DdcFnSig {
            return_type: ret.into(),
            name: FunctionId(name.into()),
            params,
        }
    }

    fn simple_fn(name: &str, body: &str) -> DdcFn {
        DdcFn {
            annotations: vec![],
            id: FunctionId(name.into()),
            is_readonly: false,
            has_contract_block: true,
            sig: fn_sig(name, vec![param("Order", "order")], "void"),
            contract: DeclaredContract::default(),
            body: DdcBody::Rust(body.into()),
        }
    }

    // ── DdcFn → fn ───────────────────────────────────────────────────────────

    #[test]
    fn fn_signature_conversion() {
        // C# params → Rust params, void → ()
        let f = simple_fn("process", "{ }");
        let ts = gen_fn(&f).unwrap().to_string();
        assert!(ts.contains("fn process"), "fn name: {ts}");
        assert!(ts.contains("order : Order"), "param: {ts}");
        assert!(ts.contains("-> ()"), "ret: {ts}");
    }

    #[test]
    fn fn_optional_return() {
        let f = DdcFn {
            annotations: vec![],
            id: FunctionId("find".into()),
            is_readonly: false,
            has_contract_block: true,
            sig: fn_sig("find", vec![param("UserId", "id")], "Optional<User>"),
            contract: DeclaredContract::default(),
            body: DdcBody::Rust("{ todo!() }".into()),
        };
        let ts = gen_fn(&f).unwrap().to_string();
        assert!(ts.contains("Option < User >") || ts.contains("Option<User>"), "{ts}");
    }

    // ── alias クロージャ挿入 ─────────────────────────────────────────────────

    #[test]
    fn alias_closure_scalar() {
        let f = DdcFn {
            annotations: vec![],
            id: FunctionId("get_city".into()),
            is_readonly: false,
            has_contract_block: true,
            sig: fn_sig("get_city", vec![param("Order", "order")], "void"),
            contract: DeclaredContract {
                read: vec![ReadPath {
                    path: StructurePath("order.customer.city".into()),
                    alias: Some("city".into()),
                }],
                ..Default::default()
            },
            body: DdcBody::Rust("{ let _ = city(); }".into()),
        };
        let ts = gen_fn(&f).unwrap().to_string();
        // let city = || &order.customer.city; が body 先頭に挿入されている
        assert!(ts.contains("let city"), "closure: {ts}");
        assert!(ts.contains("order . customer . city") || ts.contains("order.customer.city"), "{ts}");
    }

    #[test]
    fn alias_closure_collection() {
        let f = DdcFn {
            annotations: vec![],
            id: FunctionId("list_items".into()),
            is_readonly: false,
            has_contract_block: true,
            sig: fn_sig("list_items", vec![param("Order", "order")], "void"),
            contract: DeclaredContract {
                read: vec![ReadPath {
                    path: StructurePath("order.items[]".into()),
                    alias: Some("items".into()),
                }],
                ..Default::default()
            },
            body: DdcBody::Rust("{ let _ = items(); }".into()),
        };
        let ts = gen_fn(&f).unwrap().to_string();
        assert!(ts.contains("let items"), "{ts}");
        assert!(ts.contains("iter"), "{ts}");
    }

    // ── Contract → trait ─────────────────────────────────────────────────────

    #[test]
    fn contract_to_trait() {
        let decl = ContractDecl {
            id: TypeId("ICartService".into()),
            envelope: DeclaredContract::default(),
            methods: vec![ContractMethod {
                id: FunctionId("total".into()),
                sig: fn_sig("total", vec![param("Cart", "cart")], "Amount"),
                contract: DeclaredContract::default(),
            }],
        };
        let ts = gen_contract(&decl).unwrap().to_string();
        assert!(ts.contains("trait ICartService"), "{ts}");
        assert!(ts.contains("fn total"), "{ts}");
        assert!(ts.contains("& self"), "{ts}");
    }

    // ── Impl → impl ──────────────────────────────────────────────────────────

    #[test]
    fn impl_contract_method() {
        let decl = ImplDecl {
            type_id: TypeId("OrderService".into()),
            contract_ids: vec![TypeId("IOrderService".into())],
            methods: vec![ImplMethod {
                id: FunctionId("process".into()),
                sig: fn_sig("process", vec![param("Order", "order")], "void"),
                kind: ImplMethodKind::ContractImpl {
                    contract_refs: vec![FunctionId("IOrderService.process".into())],
                },
                body: DdcBody::Rust("{ }".into()),
            }],
        };
        let ts = gen_impl(&decl).unwrap().to_string();
        assert!(ts.contains("impl IOrderService for OrderService"), "{ts}");
        assert!(ts.contains("fn process"), "{ts}");
    }

    #[test]
    fn impl_own_contract_method() {
        let decl = ImplDecl {
            type_id: TypeId("FooService".into()),
            contract_ids: vec![],
            methods: vec![ImplMethod {
                id: FunctionId("helper".into()),
                sig: fn_sig("helper", vec![], "void"),
                kind: ImplMethodKind::OwnContract {
                    contract: DeclaredContract::default(),
                },
                body: DdcBody::Rust("{ }".into()),
            }],
        };
        let ts = gen_impl(&decl).unwrap().to_string();
        assert!(ts.contains("impl FooService"), "{ts}");
        assert!(ts.contains("fn helper"), "{ts}");
    }

    // ── codegen 全体 ─────────────────────────────────────────────────────────

    #[test]
    fn codegen_preamble() {
        let file = DdcFile::default();
        let ts = codegen(&file).unwrap().to_string();
        assert!(ts.contains("type Optional"), "{ts}");
        assert!(ts.contains("type List"), "{ts}");
    }

    #[test]
    fn codegen_passthrough() {
        let file = DdcFile {
            items: vec![DdcItem::RustPassthrough("struct Foo;".into())],
        };
        let ts = codegen(&file).unwrap().to_string();
        assert!(ts.contains("struct Foo"), "{ts}");
    }
}
