# DDC on Rust — 実装計画

Tech-onRust.md §8.3 のロードマップに基づく実施計画。
各フェーズの完了条件は `cargo test` 全通過。

---

## フェーズ構成

```
フェーズ 1  ddc-syntax パーサー                     ✅ 完了
    ├── フェーズ 2  DependencyGraph 構築              ✅ 完了
    │         └── フェーズ 3  LFP + readonly/kernel 検証  ✅ 完了
    └── フェーズ 4  BodyAnalyzer（syn body 検証）     ✅ 完了
                           ↓ フェーズ 1〜4 完了後
                  フェーズ 5  ddc-codegen             ✅ 完了
                           ↓
                  フェーズ 6  型レベル Write パス (`::`)  ✅ 完了
```

| フェーズ | クレート | 主な成果物 | 前提 | 状態 |
|----------|----------|-----------|------|------|
| 1: ddc-syntax パーサー | `ddc-syntax` | `parse_ddc_file`, Lexer, 15 テスト | なし | ✅ 完了 |
| 2: DependencyGraph | `ddc-core` | `DependencyGraph`（ノード・エッジ・kernel フラグ） | 1 | ✅ 完了 |
| 3: LFP + 検証 | `ddc-core` | `ContractStore::effective`, `validate_*` | 2 | ✅ 完了 |
| 4: BodyAnalyzer | `ddc-build` | syn body 検証、エイリアス解決 | 1 のみ | ✅ 完了 |
| 5: ddc-codegen | `ddc-codegen` | `TokenStream` 生成、alias クロージャ挿入 | 1〜4 | ✅ 完了 |
| 6: 型レベル Write パス | 全クレート | `::Type.field` 構文・LFP 伝播・BodyAnalyzer 拡張・契約ブロック必須化 | 1〜5 | ✅ 完了 |

---

## フェーズ 1: ddc-syntax パーサー ✅

### 成果物

| ファイル | 内容 |
|----------|------|
| `crate/ddc-syntax/src/lexer.rs` | Lexer・Token 定義、`read_brace_block` / `read_paren_block` / `read_rust_passthrough` |
| `crate/ddc-syntax/src/parser.rs` | `parse_ddc_file` 完全実装、12 テスト内包 |
| `crate/ddc-syntax/src/lib.rs` | `mod lexer` 追加 |

### テスト（12 件）

| テスト名 | 内容 |
|----------|------|
| `passthrough_struct` | `struct` → `RustPassthrough` |
| `passthrough_use` | `use` → `RustPassthrough` |
| `minimal_fn` | 最小 DdcFn（全セクション省略） |
| `fn_no_contract_block` | 契約ブロックなし → 空契約 |
| `full_contract_sections` | Read/Write/Throw 全セクション |
| `alias_in_read` | `path as alias` エイリアス |
| `readonly_fn` | `readonly` + `Optional<T>` + `Call:` |
| `kernel_fn` | `[kernel]` + `Throw:` + unsafe body |
| `contract_decl` | `contract` 宣言 + envelope + メソッド |
| `class_decl` | `class` 宣言 + `: ContractId.method`（`contract_ids[0]`、`contract_refs[0]` 確認） |
| `multiple_items` | RustPassthrough + DdcFn 混在 |
| `multi_entry_comma` | コンマ区切り複数エントリー |
| `own_contract_method` | class 内 `( ... )` 独自契約メソッド（`OwnContract` kind） |
| `multi_contract_class` | `class Foo : IBar, IBaz`（`contract_ids.len() == 2`） |
| `multi_contract_ref` | `: IReader.read, ICache.read`（`contract_refs.len() == 2`） |

### 設計決定

- **`ImplMethodKind` enum**: `ContractImpl { contract_refs: Vec<FunctionId> }`（複数 contract 同名メソッドの暗黙的実装を表現）と `OwnContract { contract: DeclaredContract }`（private helper 等）の排他 enum — Tech-onRust.md §4.6、§11.17、§11.18 に反映
- **複数 contract 対応**: `ImplDecl.contract_ids: Vec<TypeId>`、`ContractImpl.contract_refs: Vec<FunctionId>` — C# 明示的実装は DDC/Rust で非対応（§11.18）
- **外部 parser crate 不使用**（`ddc-syntax` は外部依存ゼロ原則）
- **手書き再帰降下パーサー** + Lexer 分離（行番号トラッキング）
- **契約ブロックのエントリー区切りは `,`**（改行は任意空白扱い）— Tech-onRust.md §3.3 に反映済み
- **`StructurePath` は文字列保持**（意味解析は ddc-core 担当）
- **`[kernel]` + `Read:`/`Write:` の整合性検証はフェーズ 3**（パーサーはパスのみ）

---

## フェーズ 2: DependencyGraph 構築 ✅

**クレート:** `ddc-core`  
**対象ファイル:** `crate/ddc-core/src/graph.rs`, `crate/ddc-core/src/store.rs`

### 設計決定

| 項目 | 決定 | 理由 |
|------|------|------|
| `nodes` 値型 | `DeclaredContract`（旧 `DdcFn`） | Phase 3 の LFP が必要とするのは宣言契約のみ。`ContractMethod` も同型で統一登録できる |
| `edges` 値型 | `HashSet<FunctionId>`（旧 `Vec`） | 重複辺を自動排除。Tarjan SCC は順序不問のため問題なし |
| `ContractMethod` の扱い | 修飾名 `ContractId.method_id` で `nodes` + `edges` に登録 | `ImplMethod.contract_ref`（修飾名）と自然に照合。異なる Contract の同名メソッドを区別できる |
| `ImplMethod` の扱い | `OwnContract` のみ修飾名 `TypeId.method_id` で登録。`ContractImpl` は登録しない | `ContractImpl` は contract_refs 先の ContractMethod が契約を保持するため重複登録不要 |
| `ContractDecl.envelope` の扱い | 登録しない | メソッド全体への制約上限であり、グラフノードではない。検証は Phase 3 担当 |
| 未解決 callee | 辺を登録する（nodes 不在でも可） | グラフ構築は宣言ベース。未登録ノードへの辺は許容し、LFP 計算時に無視される |
| リーフノード保証 | `edges` に空 `HashSet` を必ず登録 | Phase 3 で `edges.get(id)` が `None` にならないことを保証 |

### 実装内容

1. `DependencyGraph` 構造体の型変更（`nodes`, `edges`, `kernels`, `readonly`）
2. `DependencyGraph::build<'a>(files: impl Iterator<Item = &'a DdcFile>) -> Self`
   - `DdcFn` → `nodes`, `kernels`（`[kernel]` なら）, `readonly`（`is_readonly` なら）, `edges`（`contract.call` から）
   - `ContractDecl.methods` → 各 `ContractMethod` を `nodes` と `edges` に登録
   - `ImplDecl.methods` → `OwnContract` は修飾名 `TypeId.method_id` で `nodes` + `edges` 登録。`ContractImpl` はスキップ
   - `RustPassthrough` → スキップ
3. `ContractStore::from_files` → `DependencyGraph::build(files)` 呼び出しに置換

### テスト（8 件、`graph.rs` 内）

| テスト名 | 内容 |
|----------|------|
| `empty_file` | 空 DdcFile → 空グラフ |
| `single_fn_no_calls` | 呼び出しなし DdcFn → nodes 1件、edges 空 HashSet |
| `call_edge` | A が B を Call: 宣言 → `edges[A]` に B。B が nodes 未登録でも辺は作られる |
| `kernel_flag` | `[kernel]` アノテーション → `kernels` に登録、`nodes` にも登録 |
| `multi_file` | 2つの DdcFile → 全 nodes 合算 |
| `dedup_call` | 同一 callee が Call: に2回 → エッジ重複なし（HashSet） |
| `contract_method_registered` | ContractMethod → `nodes` に登録、Contract.call → `edges` に辺 |
| `impl_own_contract_registered` | ImplMethod(OwnContract) → 修飾名 `TypeId.method_id` で `nodes` に登録 |

---

## フェーズ 3: LFP + readonly/kernel 検証 ✅

**クレート:** `ddc-core`  
**対象ファイル:** `crate/ddc-core/src/error.rs`, `crate/ddc-core/src/graph.rs`, `crate/ddc-core/src/store.rs`

### 設計決定

| 項目 | 決定 | 理由 |
|------|------|------|
| `UndeclaredPathAccess/Call/Throw` 削除 | `ValidationErrorKind` から3バリアントを削除 | body レベルの検証は `BodyUndeclared*` が担当。ContractStore レベルでの "envelope excess" は DDC/Rust 構造的に不可能（§11.19） |
| `validate_*` の戻り型 | `Vec<ValidationError>`（空 = エラーなし） | 複数エラーを一括報告。`Result<(), E>` では1件目で終了してしまう |
| `readonly` フィールド | `DependencyGraph` に追加 | validate_readonly が O(1) で読み出せる。Phase 2 実装で追加済み |
| LFP 計算方式 | Tarjan SCC + トポロジカル順での反復 | 相互再帰（SCC）を正しく扱うための標準的アプローチ |
| `[kernel]` cut-point | `Read`/`Write` は伝播しない、`Throw` は伝播する | §10.1 仕様通り。kernel 境界の外に副作用詳細を露出しない |

### 実装内容

0. **`error.rs` クリーンアップ**: `UndeclaredPathAccess`, `UndeclaredCall`, `UndeclaredThrow` を `ValidationErrorKind` から削除
1. **Tarjan SCC** でコール依存グラフの強連結成分を分解（`graph.rs` 内のプライベート関数）
2. **最小不動点（LFP）** をトポロジカル順に反復計算（`store.rs` の `compute_all`）
   - `effective.Read(f)` / `effective.Write(f)` / `effective.Throw(f)` を §6.2 の定義通りに展開
   - `[kernel]` は cut-point: `Read`/`Write` を伝播しない、`Throw` は伝播する
3. `ContractStore::effective()` — `OnceLock` で初回呼び出し時にキャッシュ（Lazy LFP）
4. `ContractStore::validate_readonly() -> Vec<ValidationError>` — `readonly` 関数の `effective.Write` が非空ならエラー
5. `ContractStore::validate_kernel_boundary() -> Vec<ValidationError>` — `[kernel]` 関数に `Read:`/`Write:`/`Call:` 宣言があればエラー（§12.6）

### テスト（8 件、`store.rs` 内）

| テスト名 | 内容 |
|----------|------|
| `effective_single_fn` | 呼び出しなし関数の実効契約 = 宣言契約 |
| `effective_propagation` | A → B の Call チェーンで B の Write が A に伝播する |
| `effective_kernel_cut` | A → [kernel]B → C: A の effective.Write に C の Write が含まれない |
| `effective_cycle` | A ⇄ B の相互再帰（SCC）でも LFP が停留する |
| `validate_readonly_ok` | readonly 関数の実効契約に Write なし → errors 空 |
| `validate_readonly_fail` | readonly 関数の実効契約に Write あり → エラー1件 |
| `validate_kernel_boundary_ok` | [kernel] 関数に Read:/Write:/Call: なし → errors 空 |
| `validate_kernel_boundary_fail` | [kernel] 関数に Read: あり → エラー1件 |

---

## フェーズ 4: BodyAnalyzer ✅

**クレート:** `ddc-build`  
**対象ファイル:** `crate/ddc-build/src/analyzer.rs`

### 実装内容

`syn::visit::Visit` を実装し、`syn::parse_str::<syn::Block>()` で body AST を走査:

| 検証項目 | `ValidationErrorKind` |
|---------|----------------------|
| フィールドアクセスが `Read:`/`Write:` に宣言されているか | `BodyUndeclaredRead` / `BodyUndeclaredWrite` |
| 関数呼び出しが `Call:` に宣言されているか | `BodyUndeclaredCall` |
| 例外型が `Throw:` に宣言されているか | `BodyUndeclaredThrow` |
| 非 `[kernel]` 関数の `unsafe` ブロック | `BodyUnsafeBlock` |
| `panic!` / `unwrap()` / `expect()` の使用 | `BodyForbiddenMacro` |

エイリアス解析は `ReadPath.alias` から直接取得（推論不要）。

### テスト方針

- `Read:` 未宣言パスへのアクセス → エラー
- `Write:` 未宣言パスへの代入 → エラー
- `Call:` 未宣言関数の呼び出し → エラー
- `[kernel]` 関数での `unsafe` → OK
- 非 `[kernel]` での `unsafe` → エラー
- `panic!` → エラー
- エイリアス経由アクセスの正常解決

---

## フェーズ 5: ddc-codegen ✅

**クレート:** `ddc-codegen`  
**対象ファイル:** `crate/ddc-codegen/src/gen.rs`（現在 `todo!` のみ）

### 実装内容

`DdcFile` の各 `DdcItem` を `TokenStream` に変換:

| アイテム | 変換内容 |
|---------|---------|
| `RustPassthrough(text)` | テキストを `TokenStream` にパースして emit |
| `DdcFn` | C# スタイル sig → Rust `fn` に変換（型名変換 + 引数順反転）、エイリアスクロージャを body 冒頭に挿入 |
| `Contract` | Rust `trait` として emit（メソッドシグネチャのみ） |
| `Impl` | Rust `impl` ブロックとして emit |

型名変換（`sig.rs` に実装済み）:
- `Optional<T>` → `Option<T>`
- `List<T>` → `Vec<T>`
- `void` → `()`

Read エイリアスクロージャ生成（§7 参照）:

| パス形式 | 宣言例 | 生成バインディング |
|---------|--------|----------------|
| スカラー | `order.customer.city as city` | `let city = \|\| &order.customer.city;` |
| コレクション末尾 | `order.items[] as items` | `let items = \|\| order.items.iter();` |
| 射影コレクション | `order.items[].sku as sku` | `let sku = \|\| order.items.iter().map(\|n\| &n.sku);` |

### テスト方針

- `DdcFn` の Rust コード生成（型変換・引数順）
- エイリアスクロージャの挿入
- `Contract` → `trait` 生成
- `Impl` → `impl` 生成
- 生成コードが `cargo check` を通過するか（統合テスト）

---

## 未解決課題（将来対応）

| 課題 | 記載箇所 |
|------|---------|
| ソースマップ生成（rustc エラーの `.ddc` 行番号対応） | §11.13 |
| rayon による独立 SCC の並列 LFP 計算 | §8.3 パフォーマンス設計 |
| cross-file エッジ分割（変更波及のないサブグラフの再計算スキップ） | §8.3 パフォーマンス設計 |

---

## フェーズ 6: 型レベル Write パス (`::Type.field`) 🔲

**仕様参照:** Tech.md §6.7

### 背景

`new_timer() { Timer { elapsed_ms: 0 } }` のようなコンストラクタ関数は、値レベルでは構築と代入が等価であるにもかかわらず、現行の `Write:` パスが変数レベル参照しか持たないため、構築依存（`Timer.elapsed_ms` への構造カップリング）を宣言・検証できない。また、宣言されていなくてもコンパイルエラーにならない。

### 設計決定

| 決定事項 | 内容 |
|----------|------|
| 型レベル Write パス構文 | `Write: ::TypeName.field`（`::` プレフィックスで型参照を明示） |
| 変数パスとの区別 | `::` の有無のみ。命名慣習に依存しない構文的区別 |
| LFP 伝播 | 型レベルパスも通常 `Write:` パスと同じく保守的に伝播する（可能性ベース） |
| `readonly` との関係 | `::` パスも `ReadonlyViolation` の対象。例外規則なし |
| 契約ブロックの必須化 | DDC 関数には `()` が必須。省略はコンパイルエラー |

### 実装内容

| 対象ファイル | 変更内容 |
|-------------|----------|
| `ddc-syntax/src/ast.rs` | `DeclaredContract.write` を `Vec<WritePath>` に変更（`WritePath` に `is_type_level: bool` 追加）、または `DeclaredContract.type_level_write: Vec<StructurePath>` フィールド追加。`DdcFn.has_contract_block: bool` 追加 |
| `ddc-syntax/src/parser.rs` | `parse_contract_block` に `::TypeName.field` パース追加。`parse_ddc_fn` で契約ブロック省略時をエラーに変更 |
| `ddc-core/src/graph.rs` | `DependencyGraph::build` で型レベル Write を通常 Write と同一経路で `nodes` に格納 |
| `ddc-build/src/analyzer.rs` | `visit_expr_struct` を追加。`ExprStruct` のフィールド名と `::TypeName.field` 宣言を照合 |
| `ddc-build/src/driver.rs` | `has_contract_block == false` の DdcFn をエラー報告 |

### テスト方針

- `Write: ::Timer.elapsed_ms` のパース・AST 表現
- 型レベル Write の LFP 伝播（callee の `::T.field` が caller の effective.Write に入る）
- `readonly` 関数が `::T.field` を Write する → `ReadonlyViolation`
- `BodyAnalyzer`: `Timer { elapsed_ms: 0 }` を `Write: ::Timer.elapsed_ms` 未宣言で検出
- 契約ブロック省略 → コンパイルエラー