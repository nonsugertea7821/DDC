// Call 依存グラフ（Tech-onRust.md §6.3 参照）

use std::collections::{HashMap, HashSet};
use ddc_syntax::{DdcAnnotation, DdcFile, DdcItem, DeclaredContract, FunctionId, ImplMethodKind};

/// Call 依存グラフの中間表現（Semantic IR）。
/// ノード = FunctionId → DeclaredContract（DdcFn + ContractMethod の両方を格納）。
/// エッジ = Call 依存辺（HashSet で重複辺を自動排除）。
/// [kernel] フラグを持つノードは dependency propagation cut point として扱われる。
pub(crate) struct DependencyGraph {
    /// DdcFn・ContractMethod・ImplMethod(OwnContract) を統一格納。
    /// ImplMethod(ContractImpl) は除外（contract_ref 先の ContractMethod が保持）。
    pub(crate) nodes: HashMap<FunctionId, DeclaredContract>,
    /// HashSet で重複辺を自動排除。Tarjan SCC は順序不問。
    pub(crate) edges: HashMap<FunctionId, HashSet<FunctionId>>,
    pub(crate) kernels: HashSet<FunctionId>,
    pub(crate) readonly: HashSet<FunctionId>,
}

impl DependencyGraph {
    pub(crate) fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            kernels: HashSet::new(),
            readonly: HashSet::new(),
        }
    }

    /// 複数の DdcFile からグラフを構築する。
    ///
    /// - `DdcFn` と `ContractMethod` を `nodes` に登録する。
    /// - `ImplMethod` は登録しない（契約は `contract_ref` で指示した ContractMethod に委譲）。
    /// - すべての `nodes` に `edges` の空エントリを保証（Phase 3 の LFP で `.get()` が `None` にならないように）。
    pub(crate) fn build<'a>(files: impl Iterator<Item = &'a DdcFile>) -> Self {
        let mut graph = Self::new();

        for file in files {
            for item in &file.items {
                match item {
                    DdcItem::DdcFn(f) => {
                        // リーフノード保証: 空エントリを先に登録
                        graph.edges.entry(f.id.clone()).or_default();
                        graph.nodes.insert(f.id.clone(), f.contract.clone());
                        // [kernel] フラグ
                        if f.annotations.iter().any(|a| *a == DdcAnnotation::Kernel) {
                            graph.kernels.insert(f.id.clone());
                        }
                        if f.is_readonly {
                            graph.readonly.insert(f.id.clone());
                        }
                        // Call 辺（HashSet なので重複は自動排除）
                        for callee in &f.contract.call {
                            graph.edges
                                .entry(f.id.clone())
                                .or_default()
                                .insert(callee.clone());
                        }
                    }
                    DdcItem::Contract(c) => {
                        // ContractMethod を修飾名（ContractId.method_id）で nodes + edges に登録。
                        // 修飾名にすることで ImplMethod.contract_ref（修飾名）と自然に照合できる。
                        // ContractMethod に annotations フィールドはないため kernels 対象外。
                        for method in &c.methods {
                            let qualified = FunctionId(format!("{}.{}", c.id.0, method.id.0));
                            graph.edges.entry(qualified.clone()).or_default();
                            graph.nodes.insert(qualified.clone(), method.contract.clone());
                            for callee in &method.contract.call {
                                graph.edges
                                    .entry(qualified.clone())
                                    .or_default()
                                    .insert(callee.clone());
                            }
                        }
                    }
                    DdcItem::Impl(decl) => {
                        // OwnContract メソッドは修飾名（TypeId.method_id）でノード・エッジを登録
                        // ContractImpl メソッドは contract_ref 先の ContractMethod が保持しているためスキップ
                        for method in &decl.methods {
                            if let ImplMethodKind::OwnContract { contract } = &method.kind {
                                let qualified =
                                    FunctionId(format!("{}.{}", decl.type_id.0, method.id.0));
                                graph.edges.entry(qualified.clone()).or_default();
                                graph.nodes.insert(qualified.clone(), contract.clone());
                                for callee in &contract.call {
                                    graph.edges
                                        .entry(qualified.clone())
                                        .or_default()
                                        .insert(callee.clone());
                                }
                            }
                        }
                    }
                    // RustPassthrough はグラフに無関係
                    DdcItem::RustPassthrough(_) => {}
                }
            }
        }

        graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ddc_syntax::{
        ContractDecl, ContractMethod, DdcAnnotation, DdcBody, DdcFn, DdcFnSig,
        DdcFile, DdcItem, DeclaredContract, FunctionId, ImplDecl, ImplMethod,
        ImplMethodKind, TypeId,
    };

    fn make_fn(name: &str, calls: &[&str], kernel: bool) -> DdcFn {
        DdcFn {
            annotations: if kernel { vec![DdcAnnotation::Kernel] } else { vec![] },
            id: FunctionId(name.to_string()),
            is_readonly: false,
            has_contract_block: true,
            sig: DdcFnSig {
                return_type: "void".to_string(),
                name: FunctionId(name.to_string()),
                params: vec![],
            },
            contract: DeclaredContract {
                call: calls.iter().map(|s| FunctionId(s.to_string())).collect(),
                ..Default::default()
            },
            body: DdcBody::Rust("{}".to_string()),
        }
    }

    #[test]
    fn empty_file() {
        let file = DdcFile::default();
        let g = DependencyGraph::build(std::iter::once(&file));
        assert!(g.nodes.is_empty());
        assert!(g.edges.is_empty());
        assert!(g.kernels.is_empty());
    }

    #[test]
    fn single_fn_no_calls() {
        let f = make_fn("foo", &[], false);
        let file = DdcFile { items: vec![DdcItem::DdcFn(f)] };
        let g = DependencyGraph::build(std::iter::once(&file));
        assert!(g.nodes.contains_key(&FunctionId("foo".into())));
        assert_eq!(g.edges[&FunctionId("foo".into())].len(), 0);
        assert!(g.kernels.is_empty());
    }

    #[test]
    fn call_edge() {
        let a = make_fn("a", &["b"], false);
        let file = DdcFile { items: vec![DdcItem::DdcFn(a)] };
        let g = DependencyGraph::build(std::iter::once(&file));
        assert!(g.edges[&FunctionId("a".into())].contains(&FunctionId("b".into())));
        // b は nodes 未登録でも辺は存在する（未解決検証は Phase 3）
        assert!(!g.nodes.contains_key(&FunctionId("b".into())));
    }

    #[test]
    fn kernel_flag() {
        let f = make_fn("k", &[], true);
        let file = DdcFile { items: vec![DdcItem::DdcFn(f)] };
        let g = DependencyGraph::build(std::iter::once(&file));
        assert!(g.kernels.contains(&FunctionId("k".into())));
        assert!(g.nodes.contains_key(&FunctionId("k".into())));
    }

    #[test]
    fn multi_file() {
        let f1 = make_fn("foo", &[], false);
        let f2 = make_fn("bar", &[], false);
        let file1 = DdcFile { items: vec![DdcItem::DdcFn(f1)] };
        let file2 = DdcFile { items: vec![DdcItem::DdcFn(f2)] };
        let files = [file1, file2];
        let g = DependencyGraph::build(files.iter());
        assert!(g.nodes.contains_key(&FunctionId("foo".into())));
        assert!(g.nodes.contains_key(&FunctionId("bar".into())));
    }

    #[test]
    fn dedup_call() {
        // 同一 callee が 2 回登録されても辺は 1 本
        let f = make_fn("a", &["b", "b"], false);
        let file = DdcFile { items: vec![DdcItem::DdcFn(f)] };
        let g = DependencyGraph::build(std::iter::once(&file));
        assert_eq!(g.edges[&FunctionId("a".into())].len(), 1);
    }

    #[test]
    fn contract_method_registered() {
        // ContractMethod は修飾名（ContractId.method_id）で登録される。
        // ImplMethod.contract_ref = "IRepository.load" と自然に照合するため。
        let method = ContractMethod {
            id: FunctionId("load".into()),
            sig: DdcFnSig {
                return_type: "void".into(),
                name: FunctionId("load".into()),
                params: vec![],
            },
            contract: DeclaredContract {
                call: vec![FunctionId("db_query".into())],
                ..Default::default()
            },
        };
        let decl = ContractDecl {
            id: TypeId("IRepository".into()),
            envelope: DeclaredContract::default(),
            methods: vec![method],
        };
        let file = DdcFile { items: vec![DdcItem::Contract(decl)] };
        let g = DependencyGraph::build(std::iter::once(&file));
        let key = FunctionId("IRepository.load".into());
        assert!(g.nodes.contains_key(&key));
        assert!(g.edges[&key].contains(&FunctionId("db_query".into())));
        // 非修飾名は登録されない
        assert!(!g.nodes.contains_key(&FunctionId("load".into())));
    }

    #[test]
    fn impl_own_contract_registered() {
        // OwnContract メソッドは修飾名（TypeId.method_id）で nodes/edges に登録される
        let method = ImplMethod {
            id: FunctionId("helper".into()),
            sig: DdcFnSig {
                return_type: "void".into(),
                name: FunctionId("helper".into()),
                params: vec![],
            },
            kind: ImplMethodKind::OwnContract {
                contract: DeclaredContract {
                    call: vec![FunctionId("db_query".into())],
                    ..Default::default()
                },
            },
            body: DdcBody::Rust("{}".to_string()),
        };
        let decl = ImplDecl {
            type_id: TypeId("SqlRepository".into()),
            contract_ids: vec![TypeId("IRepository".into())],
            methods: vec![method],
        };
        let file = DdcFile { items: vec![DdcItem::Impl(decl)] };
        let g = DependencyGraph::build(std::iter::once(&file));

        let key = FunctionId("SqlRepository.helper".into());
        assert!(g.nodes.contains_key(&key));
        assert!(g.edges[&key].contains(&FunctionId("db_query".into())));
        // 非修飾名は登録されない
        assert!(!g.nodes.contains_key(&FunctionId("helper".into())));
    }
}
