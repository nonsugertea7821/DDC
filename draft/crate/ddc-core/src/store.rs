// ContractStore（Tech-onRust.md §6.3 参照）
// フェーズ 3: Tarjan SCC + LFP + readonly/kernel 検証

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use ddc_syntax::{DdcFile, FunctionId};
use crate::{EffectiveContract, ValidationError, ValidationErrorKind};
use crate::graph::DependencyGraph;

pub struct ContractStore {
    graph:     DependencyGraph,
    effective: OnceLock<HashMap<FunctionId, EffectiveContract>>,
}

// ─── Tarjan SCC ──────────────────────────────────────────────────────────────

struct TarjanState {
    counter:  usize,
    stack:    Vec<FunctionId>,
    on_stack: HashSet<FunctionId>,
    index:    HashMap<FunctionId, usize>,
    lowlink:  HashMap<FunctionId, usize>,
    output:   Vec<Vec<FunctionId>>,
}

fn strongconnect(v: &FunctionId, graph: &DependencyGraph, s: &mut TarjanState) {
    s.index.insert(v.clone(), s.counter);
    s.lowlink.insert(v.clone(), s.counter);
    s.counter += 1;
    s.stack.push(v.clone());
    s.on_stack.insert(v.clone());

    // 隣接ノードを先に収集して borrow 競合を回避
    let neighbors: Vec<FunctionId> = graph.edges
        .get(v)
        .map(|ns| ns.iter().cloned().collect())
        .unwrap_or_default();

    for w in &neighbors {
        if !graph.nodes.contains_key(w) { continue; }  // 未登録ノードはスキップ
        if !s.index.contains_key(w) {
            strongconnect(w, graph, s);
            let wll = *s.lowlink.get(w).unwrap();           // Copy → 借用終了
            let vll = s.lowlink.get_mut(v).unwrap();
            *vll = (*vll).min(wll);
        } else if s.on_stack.contains(w) {
            let widx = *s.index.get(w).unwrap();            // Copy → 借用終了
            let vll = s.lowlink.get_mut(v).unwrap();
            *vll = (*vll).min(widx);
        }
    }

    if s.lowlink.get(v) == s.index.get(v) {
        let mut scc = vec![];
        loop {
            let w = s.stack.pop().unwrap();
            s.on_stack.remove(&w);
            let done = w == *v;
            scc.push(w);
            if done { break; }
        }
        s.output.push(scc);
    }
}

/// Tarjan SCC を実行し、逆トポロジカル順（callee の SCC が先）の SCC リストを返す。
fn tarjan_sccs(graph: &DependencyGraph) -> Vec<Vec<FunctionId>> {
    let mut s = TarjanState {
        counter:  0,
        stack:    Vec::new(),
        on_stack: HashSet::new(),
        index:    HashMap::new(),
        lowlink:  HashMap::new(),
        output:   Vec::new(),
    };
    for v in graph.nodes.keys() {
        if !s.index.contains_key(v) {
            strongconnect(v, graph, &mut s);
        }
    }
    s.output
}

// ─── LFP 計算 ────────────────────────────────────────────────────────────────

/// Tarjan SCC + トポロジカル順 LFP で全関数の実効契約を計算する。
/// - Read/Write を伝播する。
/// - [kernel] は Read/Write の伝播を遮断する（§10.1）。
fn compute_all(graph: &DependencyGraph) -> HashMap<FunctionId, EffectiveContract> {
    // Tarjan 返却順 = 逆トポロジカル順（callee の SCC が先）
    let sccs = tarjan_sccs(graph);
    let mut result: HashMap<FunctionId, EffectiveContract> = HashMap::new();

    for scc in &sccs {
        let scc_set: HashSet<&FunctionId> = scc.iter().collect();

        // Step 1: 宣言契約 + SCC 外 callee の実効契約で初期化
        for m in scc {
            let Some(declared) = graph.nodes.get(m) else { continue };
            let mut eff = EffectiveContract {
                read:  declared.read.iter().map(|rp| rp.path.clone()).collect(),
                write: declared.write.iter().cloned().collect(),
                call:  declared.call.iter().cloned().collect(),
            };
            if let Some(callees) = graph.edges.get(m) {
                for callee in callees {
                    if scc_set.contains(callee) { continue; }
                    let Some(callee_eff) = result.get(callee) else { continue };
                    if !graph.kernels.contains(callee) {
                        // non-kernel: Read/Write も伝播
                        eff.read.extend(callee_eff.read.iter().cloned());
                        eff.write.extend(callee_eff.write.iter().cloned());
                    }
                }
            }
            result.insert(m.clone(), eff);
        }

        // Step 2: SCC 内部を不動点まで反復（相互再帰の LFP）
        loop {
            let mut changed = false;
            // スナップショットで borrow 競合を回避
            let snap: HashMap<FunctionId, EffectiveContract> = scc.iter()
                .filter_map(|id| result.get(id).map(|e| (id.clone(), e.clone())))
                .collect();
            for m in scc {
                let Some(callees) = graph.edges.get(m) else { continue };
                for callee in callees {
                    if !scc_set.contains(callee) { continue; }
                    let Some(callee_eff) = snap.get(callee) else { continue };
                    let eff = result.get_mut(m).unwrap();
                    let before = eff.read.len() + eff.write.len();
                    if !graph.kernels.contains(callee) {
                        eff.read.extend(callee_eff.read.iter().cloned());
                        eff.write.extend(callee_eff.write.iter().cloned());
                    }
                    if eff.read.len() + eff.write.len() > before {
                        changed = true;
                    }
                }
            }
            if !changed { break; }
        }
    }

    result
}

// ─── ContractStore ───────────────────────────────────────────────────────────

impl ContractStore {
    /// 複数の DdcFile から ContractStore を構築する（グラフ構築は eager）。
    /// 実効契約の計算は effective() の初回呼び出しまで遅延される（Lazy LFP）。
    pub fn from_files<'a>(files: impl Iterator<Item = &'a DdcFile>) -> Self {
        Self {
            graph:     DependencyGraph::build(files),
            effective: OnceLock::new(),
        }
    }

    /// 指定関数の実効契約を返す。
    /// 初回呼び出し時に全関数の実効契約を Tarjan SCC + LFP で計算しキャッシュする。
    pub fn effective(&self, id: &FunctionId) -> Option<&EffectiveContract> {
        let cache = self.effective.get_or_init(|| compute_all(&self.graph));
        cache.get(id)
    }

    /// readonly 推移性を検証する。
    /// readonly 関数の effective.Write が非空であれば ReadonlyViolation を返す。
    pub fn validate_readonly(&self) -> Vec<ValidationError> {
        let cache = self.effective.get_or_init(|| compute_all(&self.graph));
        let mut errors = Vec::new();
        for id in &self.graph.readonly {
            if let Some(eff) = cache.get(id) {
                if !eff.write.is_empty() {
                    let writes: Vec<&str> =
                        eff.write.iter().map(|p| p.0.as_str()).collect();
                    errors.push(ValidationError {
                        kind:     ValidationErrorKind::ReadonlyViolation,
                        function: id.clone(),
                        message:  format!(
                            "readonly 関数 '{}' の実効契約に Write が含まれます: {}",
                            id.0,
                            writes.join(", "),
                        ),
                    });
                }
            }
        }
        errors
    }

    /// [kernel] 境界制約を検証する（§12.6）。
    /// [kernel] 関数に Read:/Write:/Call: 宣言があれば KernelBoundaryViolation を返す。
    pub fn validate_kernel_boundary(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();
        for id in &self.graph.kernels {
            if let Some(declared) = self.graph.nodes.get(id) {
                if !declared.read.is_empty()
                    || !declared.write.is_empty()
                    || !declared.call.is_empty()
                {
                    errors.push(ValidationError {
                        kind:     ValidationErrorKind::KernelBoundaryViolation,
                        function: id.clone(),
                        message:  format!(
                            "[kernel] 関数 '{}' に Read:/Write:/Call: 宣言が存在します（§12.6）",
                            id.0,
                        ),
                    });
                }
            }
        }
        errors
    }

    /// [kernel] アノテーションを持つかを判定する
    pub fn is_kernel(&self, id: &FunctionId) -> bool {
        self.graph.kernels.contains(id)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ddc_syntax::{
        DdcAnnotation, DdcBody, DdcFn, DdcFnSig, DdcFile, DdcItem,
        DeclaredContract, FunctionId, ReadPath, StructurePath,
    };

    fn make_fn(name: &str, calls: &[&str]) -> DdcFn {
        DdcFn {
            annotations: vec![],
            id:          FunctionId(name.to_string()),
            is_readonly: false,
            has_contract_block: true,
            sig:         DdcFnSig {
                return_type: "void".to_string(),
                name:        FunctionId(name.to_string()),
                params:      vec![],
            },
            contract:    DeclaredContract {
                call: calls.iter().map(|s| FunctionId(s.to_string())).collect(),
                ..Default::default()
            },
            body:        DdcBody::Rust("{}".to_string()),
        }
    }

    fn with_write(mut f: DdcFn, fields: &[&str]) -> DdcFn {
        f.contract.write = fields.iter().map(|s| StructurePath(s.to_string())).collect();
        f
    }

    fn with_read(mut f: DdcFn, fields: &[&str]) -> DdcFn {
        f.contract.read = fields.iter().map(|s| ReadPath {
            path:  StructurePath(s.to_string()),
            alias: None,
        }).collect();
        f
    }

    fn as_kernel(mut f: DdcFn) -> DdcFn {
        f.annotations = vec![DdcAnnotation::Kernel];
        f
    }

    fn as_readonly(mut f: DdcFn) -> DdcFn {
        f.is_readonly = true;
        f
    }

    fn store(fns: Vec<DdcFn>) -> ContractStore {
        let file = DdcFile { items: fns.into_iter().map(DdcItem::DdcFn).collect() };
        ContractStore::from_files(std::iter::once(&file))
    }

    // ── 実効契約 ──────────────────────────────────────────────────────────────

    #[test]
    fn effective_single_fn() {
        // 呼び出しなし関数の実効契約 = 宣言契約
        let f = with_write(make_fn("f", &[]), &["order.amount"]);
        let s = store(vec![f]);
        let eff = s.effective(&FunctionId("f".into())).unwrap();
        assert!(eff.write.contains(&StructurePath("order.amount".into())));
        assert!(eff.read.is_empty());
    }

    #[test]
    fn effective_propagation() {
        // A → B: B の Write が A に伝播する
        let b = with_write(make_fn("b", &[]), &["order.amount"]);
        let a = make_fn("a", &["b"]);
        let s = store(vec![a, b]);
        let eff = s.effective(&FunctionId("a".into())).unwrap();
        assert!(eff.write.contains(&StructurePath("order.amount".into())));
    }

    #[test]
    fn effective_kernel_cut() {
        // A → [kernel]B → C: A の effective.Write に C の Write が含まれない
        // B は kernel なので Call: 宣言は §12.6 違反だが、ここでは LFP のアルゴリズムを検証する
        let c = with_write(make_fn("c", &[]), &["secret.data"]);
        let b = as_kernel(make_fn("b", &["c"]));
        let a = make_fn("a", &["b"]);
        let s = store(vec![a, b, c]);
        // A から kernel B 越しに C の Write は見えない
        let eff_a = s.effective(&FunctionId("a".into())).unwrap();
        assert!(!eff_a.write.contains(&StructurePath("secret.data".into())));
        // B 自身の実効契約には C の Write が含まれる（B はカーネル境界の内側）
        let eff_b = s.effective(&FunctionId("b".into())).unwrap();
        assert!(eff_b.write.contains(&StructurePath("secret.data".into())));
    }

    #[test]
    fn effective_cycle() {
        // A ⇄ B の相互再帰: 両者の Write が両方に伝播して LFP が停留する
        let a = with_write(make_fn("a", &["b"]), &["field_a"]);
        let b = with_write(make_fn("b", &["a"]), &["field_b"]);
        let s = store(vec![a, b]);
        let eff_a = s.effective(&FunctionId("a".into())).unwrap();
        let eff_b = s.effective(&FunctionId("b".into())).unwrap();
        assert!(eff_a.write.contains(&StructurePath("field_a".into())));
        assert!(eff_a.write.contains(&StructurePath("field_b".into())));
        assert!(eff_b.write.contains(&StructurePath("field_a".into())));
        assert!(eff_b.write.contains(&StructurePath("field_b".into())));
    }

    // ── validate_readonly ─────────────────────────────────────────────────────

    #[test]
    fn validate_readonly_ok() {
        // readonly 関数の実効契約に Write なし → errors 空
        let f = as_readonly(make_fn("reader", &[]));
        let s = store(vec![f]);
        assert!(s.validate_readonly().is_empty());
    }

    #[test]
    fn validate_readonly_fail() {
        // readonly 関数が Write を持つ callee を呼ぶ → エラー 1 件
        let writer = with_write(make_fn("writer", &[]), &["db.record"]);
        let reader = as_readonly(make_fn("reader", &["writer"]));
        let s = store(vec![reader, writer]);
        let errors = s.validate_readonly();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::ReadonlyViolation);
        assert_eq!(errors[0].function, FunctionId("reader".into()));
    }

    // ── validate_kernel_boundary ──────────────────────────────────────────────

    #[test]
    fn validate_kernel_boundary_ok() {
        // [kernel] 関数に依存宣言なし → errors 空
        let k = as_kernel(make_fn("k", &[]));
        let s = store(vec![k]);
        assert!(s.validate_kernel_boundary().is_empty());
    }

    #[test]
    fn validate_kernel_boundary_fail() {
        // [kernel] 関数に Read: あり → エラー 1 件
        let k = as_kernel(with_read(make_fn("k", &[]), &["config.value"]));
        let s = store(vec![k]);
        let errors = s.validate_kernel_boundary();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::KernelBoundaryViolation);
        assert_eq!(errors[0].function, FunctionId("k".into()));
    }
}
