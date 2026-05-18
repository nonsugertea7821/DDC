# DDC on Rust — 実装仕様書

本文書は、DDC（Declarative Dependency Control）言語の Rust 実装について定義する。
DDC の言語仕様は `Tech.md` を参照すること。本文書は「Rust 上でどう実現するか」を対象とする。

---

## 1. 位置づけ

DDC は **Rust に射影される独立言語**である。`.ddc` ファイルに記述された DDC ソースは、
ビルド時に Rust コードへコンパイルされ、ホストクレートの `build.rs` を通じて Rust のコンパイルパイプラインに組み込まれる。

```
*.ddc ファイル
    → ddc-syntax  : テキストパース → DdcFile AST
    → ddc-core    : ContractStore 構築・実効契約計算・検証
    → ddc-codegen : DdcFile → Rust TokenStream 生成
    → $OUT_DIR/ddc_generated.rs
    → include!(concat!(env!("OUT_DIR"), "/ddc_generated.rs"))
    → rustc によるコンパイル
```

### Rust との役割分担

| 役割 | 担当 |
| --- | --- |
| 型定義（`struct`, `enum`, `type`） | Rust（`.rs` ファイルに通常の Rust として記述） |
| 関数の依存契約宣言・本体 | DDC（`.ddc` ファイルに記述） |
| 関数宣言構文 | C# スタイル（戻り型が先頭、`fn` キーワードなし、`型名 引数名` 順） |
| 型式（型名・ジェネリクス） | Rust 型をベースに DDC 向けエイリアスを使用（§11 参照） |
| ビルド時の契約検証 | `ddc-build` クレート（`build.rs` から呼び出す） |

`.ddc` ファイルと `.rs` ファイルは同一プロジェクト内に共存する。
型定義は Rust 側、関数の DDC 契約は DDC 側という分担が基本となる。

---

## 2. クレート構成

ワークスペースルート: `crate/Cargo.toml`

```
ddc-syntax   依存なし（テキストパーサー・AST）
ddc-core     ddc-syntax に依存（ContractStore・検証）
ddc-codegen  ddc-syntax, proc-macro2, quote に依存（Rust コード生成）
ddc-build    ddc-syntax, ddc-core, ddc-codegen に依存（ビルドドライバー）
```

依存方向: `ddc-build → ddc-codegen → ddc-syntax`
                     `↘ ddc-core → ddc-syntax`

### 各クレートの責務

| クレート | 責務 |
| --- | --- |
| `ddc-syntax` | `.ddc` テキストのパース、AST 型定義 |
| `ddc-core` | ContractStore の構築、実効契約計算（Tarjan SCC + LFP）、検証 |
| `ddc-codegen` | `DdcFile` AST → Rust `TokenStream` 変換 |
| `ddc-build` | `build.rs` 向けエントリーポイント、`.ddc` ファイル走査・コンパイル |

---

## 3. `.ddc` ファイル形式

### 3.1 基本構造

`.ddc` ファイルはトップレベルアイテムの並びからなる。アイテムには 4 種類ある。

```
DdcItem
  ├── RustPassthrough  — Rust の型定義・use 宣言などをそのまま pass-through
  ├── DdcFn            — DDC 関数
  ├── Contract         — contract 宣言（インターフェース相当）
  └── Impl             — class 宣言（contract 実装）
```

### 3.2 Rust pass-through

Rust の `struct` / `enum` / `type` / `use` 等はそのまま Rust 構文で記述し、
codegen 時にテキストとして emit される。

```rust
// .ddc ファイル内の Rust pass-through
struct Order {
    id: OrderId,
    items: List<Item>,
    status: OrderStatus,
}
```

型定義は Rust 側に置くことが推奨されるが、`.ddc` ファイル内に共置することも許容される。

### 3.3 DDC 関数

DDC 関数は **C# スタイルの宣言構文**を採用する。`fn` キーワードは使用しない。

```
[アノテーション]
[アクセス修飾子] [readonly] 戻り型 関数名(引数リスト)
(
    Read:
        path1, path2
    Write:
        path3
    Call:
        func1()
) {
    // Rust body
}
```

**構文規則:**
- 戻り型が先頭、次に関数名、次に引数リスト（C# / Java スタイル）
- `fn` キーワードは使用しない
- パラメータは `型名 引数名`（C# スタイル。例: `Order order`）
- 戻り値なしは `void`
- 省略可能型は `Optional<T>`（`Option<T>` / `?` は使用しない）
- コレクション型は `List<T>`（`Vec<T>` は使用しない）
- `readonly` 修飾子は戻り型の前に付与する
- `[kernel]` アノテーションは関数宣言の直前の行に置く
- アクセス修飾子（`public` / `private` 等）を付与できる（公開範囲と契約制約は直交する概念）
- セクション順序: `Read:` → `Write:` → `Call:` （固定）
- セクションはすべて省略可能。省略されたセクションは空集合と等価
- セクション内のエントリは **`,`（コンマ）区切り**。末尾コンマ可。改行は任意の空白として扱う
- エントリの終端は次のセクションキーワード（`Read:` / `Write:` / `Call:`）または `)` で確定する

**例:**

```
public void process_shipping(Order order)
(
    Read:
        order.id,
        order.items[].sku
    Write:
        order.status,
        ::Exception.Message
    Call:
        ::Exception.Constructor()
) {
    // Rust body
}
```

**`Read:` エイリアス宣言例:**

```
public void process_shipping(Order order)
(
    Read:
        order.id,
        order.items[].sku as sku    // エイリアス宣言: body 内で sku() として使用できる
    Write:
        order.status
) {
    // body 内では sku() と呼び出す
    // codegen が let sku = || order.items.iter().map(|n| &n.sku); を先頭に挿入する
}
```

**`readonly` 関数:**

```
public readonly Optional<User> find(UserId id)
(
    Read:
        id.value
    Call:
        repository.load()
) {
    // Rust body
}
```

複数エントリはコンマで区切る:

```
public void transfer(Account from, Account to)
(
    Read:
        from.balance, to.id
    Write:
        from.balance, to.balance
    Call:
        ::Exception.Constructor(), notify_failure()
) {
    // Rust body
}
```

**`[kernel]` 関数:**

```
[kernel]
public void send_bytes(Conn conn)
() {
    // Rust body（unsafe 可）
    unsafe { /* ... */ }
}
```

`[kernel]` の契約ブロックは空 (`()`) のみ記述できる。`Read:` / `Write:` / `Call:` は宣言禁止。

### 3.4 contract 宣言

capability envelope とメソッド宣言を持つインターフェース相当の構文。

```
contract IRepository
(
    Read:
        id.value
)
{
    Optional<User> load(UserId id)
    (
        Read: id.value
    );
}
```

- 外側の `( ... )` は全メソッドに適用される capability envelope
- `{ ... }` 内に各メソッドとそのメソッド個別の契約 `( ... )` を宣言する

### 3.5 class 宣言

contract を実装する型の宣言。`class` キーワードを使用する。

```
// 単一 contract
class SqlRepository : IRepository {
    public Optional<User> load(UserId id) : IRepository.load {
        // Rust body
    }
}

// 複数 contract（コンマ区切り）
class FileCache : IReader, IWriter {
    public Data read(FileId id)            : IReader.read  { ... }
    public void  write(FileId id, Data d)  : IWriter.write { ... }
}

// 同名メソッドが複数 contract に存在する場合
class DualReader : IReader, ICache {
    public Data read(FileId id) : IReader.read, ICache.read {
        // Rust 上は 1 関数。両 contract のシグネチャが同一であること（Phase 3 で検証）
    }
}
```

- `class Type : Contract1, Contract2, ...` で実装対象の contract をコンマ区切りで宣言する
  - `:` は C# / Java の `class Foo : IBar` / `class Foo implements IBar` と同じ意味
  - C# の「明示的インターフェース実装」（同名メソッドを別関数で実装）は DDC / Rust では非対応。同名メソッドは 1 つの Rust 関数にまとめる（§11.18 参照）
- class メソッドには 2 種類の契約形式がある（§11.17 参照）:
  - **委譲形式**（`: ContractName.MethodName` または `: C1.m, C2.m`）— 対応する `ContractMethod` の `DeclaredContract` に委譲する。契約ブロック `( ... )` は不要。1 メソッドが複数 contract の同名メソッドを満たす場合はコンマで列挙する
  - **独自契約形式**（`( ... )` ブロック）— class メソッドが自身の契約ブロックを持つ。contract に対応しない private helper 等に使用する
- 実装は対応する contract の capability envelope を超えてはならない

契約形式の対比:

| 記述形式 | 契約の出所 | 構文 |
| --- | --- | --- |
| `DdcFn`（スタンドアロン） | 自身の `( ... )` ブロック | `void foo(T x) ( Read: ... ) { ... }` |
| class 内委譲メソッド | 参照先 contract メソッドに委譲 | `void foo(T x) : IContract.foo { ... }` |
| class 内独自契約メソッド | 自身の `( ... )` ブロック | `private void foo(T x) ( Read: ... ) { ... }` |

---

## 4. AST 型定義（`ddc-syntax`）

### 4.1 基本識別子

```rust
pub struct StructurePath(pub String);  // "order.items[].sku" など
pub struct FunctionId(pub String);     // "process_shipping" など
pub struct TypeId(pub String);         // "IRepository", "Order" など
```

すべて `Debug + Clone + PartialEq + Eq + Hash` を実装。`ContractStore` の `HashMap` キーとして利用される。

`Read:` セクションのパスエントリはエイリアス宣言（`as identifier`）を持ちうるため、`ReadPath` 型で表現する。

```rust
/// `Read:` セクションの 1 エントリ。パスと省略可能なエイリアス名を保持する。
pub struct ReadPath {
    pub path:  StructurePath,
    pub alias: Option<String>,  // `as x` 宣言が存在する場合 `Some("x")`、なければ `None`
}
```

### 4.2 宣言契約

```rust
pub struct DeclaredContract {
    pub read:  Vec<ReadPath>,       // Read: エントリ（エイリアスあり・なし両方を含む）
    pub write: Vec<StructurePath>,
    pub call:  Vec<FunctionId>,
}
```

省略されたセクションは空の `Vec` と等価。

### 4.3 DDC アノテーション

```rust
pub enum DdcAnnotation {
    Kernel,
}
```

現在定義されているアノテーションは `[kernel]` のみ。

### 4.4 DDC Body

```rust
pub enum DdcBody {
    Rust(String),  // Rust 構文による body テキスト
}
```

body は常に Rust 構文で記述する（§11.14 参照）。

### 4.5 DDC 関数

```rust
/// 関数の 1 パラメータ。C# スタイル（`型名 引数名`）で保持する。
pub struct Param {
    pub type_name:  String,   // "Order", "UserId", "Optional<User>" など
    pub param_name: String,   // "order", "id" など
}

/// DDC 関数のシグネチャを構造化して保持する。
/// パーサーは C# スタイルのテキストをこの型に分解する。
/// codegen は各フィールドを Rust スタイルに変換して emit する（§11.13 参照）。
pub struct DdcFnSig {
    pub return_type: String,       // "void", "Optional<User>", "List<Item>" など
    pub name:        FunctionId,   // 関数名
    pub params:      Vec<Param>,   // パラメータリスト（順序保持）
}

pub struct DdcFn {
    pub annotations: Vec<DdcAnnotation>,
    pub id:          FunctionId,
    pub is_readonly: bool,
    pub sig:         DdcFnSig,      // 構造化されたシグネチャ。codegen が Rust スタイルに変換して emit する（§11.13 参照）
    pub contract:    DeclaredContract,
    pub body:        DdcBody,
}
```

`DdcFnSig` は `Param` のリストとして保持された構造化シグネチャ。codegen 時に Rust スタイル（`fn load(id: UserId) -> Option<User>`）に変換して emit する（§11.13 参照）。

### 4.6 contract / impl 宣言

```rust
pub struct ContractMethod {
    pub id:       FunctionId,
    pub sig:      DdcFnSig,
    pub contract: DeclaredContract,
}

pub struct ContractDecl {
    pub id:       TypeId,
    pub envelope: DeclaredContract,
    pub methods:  Vec<ContractMethod>,
}

/// class 内メソッドの契約の出所を示す（§11.17 参照）。
pub enum ImplMethodKind {
    /// `: ContractName.method_name` または `: C1.m, C2.m` —
    /// 契約を contract メソッドに委譲する。複数の contract の同名メソッドを
    /// 1 つの Rust 関数で満たす場合はコンマ区切りで列挙する（§11.18 参照）。
    ContractImpl { contract_refs: Vec<FunctionId> },
    /// `( Read: ... )` — class メソッドが自身の契約ブロックを持つ（private helper 等）。
    OwnContract { contract: DeclaredContract },
}

pub struct ImplMethod {
    pub id:   FunctionId,
    pub sig:  DdcFnSig,
    pub kind: ImplMethodKind,
    pub body: DdcBody,
}

pub struct ImplDecl {
    pub type_id:      TypeId,
    pub contract_ids: Vec<TypeId>,  // 実装する contract のリスト（1つ以上）
    pub methods:      Vec<ImplMethod>,
}
```

### 4.7 ファイルトップレベル

```rust
pub enum DdcItem {
    RustPassthrough(String),
    DdcFn(DdcFn),
    Contract(ContractDecl),
    Impl(ImplDecl),
}

pub struct DdcFile {
    pub items: Vec<DdcItem>,
}
```

---

## 5. パースエラー（`ddc-syntax`）

```rust
pub struct ParseError {
    pub message: String,
    pub line:    Option<usize>,   // 1-based 行番号、不明な場合は None
}
```

`proc_macro2::Span` には依存しない。`ddc-syntax` は pure テキスト処理クレートであり、
proc-macro 機構とは完全に切り離されている。

エントリーポイント:

```rust
pub fn parse_ddc_file(input: &str) -> Result<DdcFile, ParseError>
```

---

## 6. ContractStore と実効契約（`ddc-core`）

### 6.1 実効契約型

```rust
pub struct EffectiveContract {
    pub read:  HashSet<StructurePath>,
    pub write: HashSet<StructurePath>,
    pub call:  HashSet<FunctionId>,
}
```

宣言契約が `Vec` であるのに対し、実効契約は和集合演算のため `HashSet` を使用する。

### 6.2 実効契約の定義

$K$ を `[kernel]` 関数の全体集合とする。

$$\text{effective.Read}(f) = \text{declared.Read}(f) \cup \bigcup_{\substack{g \in \text{Call}(f) \\ g \notin K}} \text{effective.Read}(g)$$

$$\text{effective.Write}(f) = \text{declared.Write}(f) \cup \bigcup_{\substack{g \in \text{Call}(f) \\ g \notin K}} \text{effective.Write}(g)$$
`[kernel]` 関数は **dependency propagation cut point（依存伝播切断点）** として機能する。
`[kernel]` 関数の `effective.Read` / `effective.Write` は呼び出し元に **伝播しない**。
エラー経路（`throw` / `panic`）は依存契約の合成対象外とする。

Rust 以外をホストとする実装では、`[kernel]` 内で生成された例外オブジェクトが外部へ漏れる可能性を完全には遮断できない。
これは依存契約ではなく実行時制御の問題として扱い、必要に応じて Middleware 層で捕捉・正規化し `Call` / `Write` に落として表現する。

```ddc
[kernel]
public void send_bytes(Conn conn)();

public void safe_send(Packet packet)
(
    Read:
        packet.bytes[]
    Call:
        network.send(),
        ::Exception.Constructor()
    Write:
        ::Exception.Message
) { /* middleware */ }
```

再帰・相互再帰は最小不動点（LFP）として計算する。実装は Tarjan SCC + トポロジカル順序で反復する。

### 6.3 ContractStore API

`DependencyGraph` は Call 依存グラフの中間表現（Semantic IR）として ddc-core 内に保持される。`ContractStore` はグラフを内包し、検証・クエリの API を提供する。

```rust
/// Call 依存グラフの中間表現（Semantic IR）。
/// ノード = FunctionId → DeclaredContract（DdcFn + ContractMethod の両方を格納）。
/// エッジ = Call 依存辺（HashSet で重複辺を自動排除）。
/// [kernel] フラグを持つノードは dependency propagation cut point として扱われる。
pub struct DependencyGraph {
    /// DdcFn・ContractMethod・ImplMethod(OwnContract) を統一格納。
    /// ImplMethod(ContractImpl) は除外（contract_ref 先の ContractMethod が保持）。
    /// DdcFn のキーは非修飾名（例: `process_order`）。
    /// ContractMethod のキーは修飾名（例: `IRepository.load`）— ImplMethod.contract_ref と自然に照合するため。
    nodes:   HashMap<FunctionId, DeclaredContract>,
    /// HashSet を使用し重複辺を自動排除。Tarjan SCC は順序不問のため問題なし。
    edges:   HashMap<FunctionId, HashSet<FunctionId>>,  // caller → callees
    kernels: HashSet<FunctionId>,
    readonly: HashSet<FunctionId>,  // is_readonly フラグを持つ DdcFn
}

pub struct ContractStore {
    graph:     DependencyGraph,
    effective: OnceLock<HashMap<FunctionId, EffectiveContract>>,  // 遅延評価キャッシュ
}

impl ContractStore {
    /// 複数の DdcFile から ContractStore を構築する（グラフ構築は eager）。
    /// 実効契約の計算は effective() の初回呼び出しまで遅延される（Lazy LFP）。
    pub fn from_files(files: impl Iterator<Item = &'_ DdcFile>) -> Self;

    /// 指定関数の実効契約を返す。
    /// 初回呼び出し時に全関数の実効契約を Tarjan SCC + LFP で計算しキャッシュする。
    pub fn effective(&self, id: &FunctionId) -> Option<&EffectiveContract>;

    /// readonly 推移性を検証する。
    pub fn validate_readonly(&self) -> Vec<ValidationError>;

    /// [kernel] 境界制約を検証する。
    pub fn validate_kernel_boundary(&self) -> Vec<ValidationError>;

    /// [kernel] アノテーションを持つかを判定する。
    pub fn is_kernel(&self, id: &FunctionId) -> bool;
}
```

### 6.4 検証エラー型

```rust
pub struct ValidationError {
    pub kind:     ValidationErrorKind,
    pub function: FunctionId,
    pub message:  String,
}

pub enum ValidationErrorKind {
    // 宣言契約レベルの検証（ContractStore）
    ReadonlyViolation,         // readonly 関数の effective.Write が非空（§10.1 LFP 結果）
    KernelBoundaryViolation,   // [kernel] 関数に Read:/Write:/Call: セクションが存在する（§12.6）

    // body レベルの検証（BodyAnalyzer）
    BodyUndeclaredRead,        // body が Read:/Write: に宣言されていないパスを直接読み取っている
    BodyUndeclaredWrite,       // body が Write: に宣言されていないパスを直接変更している
    BodyUndeclaredCall,        // body が Call: に宣言されていない関数を直接呼び出している
    BodyUnsafeBlock,           // 非 [kernel] 関数の body に unsafe ブロックが存在する
    BodyForbiddenMacro,        // body に panic! / unwrap() / expect() が存在する

    // エイリアス宣言レベルの検証（パース・宣言時）
    AliasNameConflict,         // エイリアス名がパラメータ名または他のエイリアス名と衝突する
    AliasOnNonReadSection,     // Write:/Call: セクションに as 修飾子が使用された
}
```

---

## 7. コード生成（`ddc-codegen`）

エントリーポイント:

```rust
pub fn codegen(file: &DdcFile) -> Result<TokenStream, CodegenError>
```

各 `DdcItem` の変換方針:

| アイテム | 変換 |
| --- | --- |
| 型エイリアス preamble | 生成ファイル冒頭に `pub type Optional<T> = Option<T>;` / `pub type List<T> = Vec<T>;` を emit（body 内での C# スタイル型名使用を有効にするため） |
| `RustPassthrough(text)` | テキストを `TokenStream` にパースして emit |
| `DdcFn` | C# スタイルの `sig` を Rust `fn` シグネチャに変換（`Optional<T>` → `Option<T>`、`List<T>` → `Vec<T>`、`void` → `()`、引数順の反転）して emit |
| `Contract` | Rust trait として emit（メソッドシグネチャのみ） |
| `Impl` | Rust `impl` ブロックとして emit |

`DdcBody::Rust(text)` の body は文字列として `TokenStream` にパースして emit する。

**Read エイリアスバインディングの生成:**

`ReadPath.alias` が `Some(name)` の場合、生成される関数 body の冒頭にクロージャバインディングを挿入する。

| パスの形式 | 宣言例 | 生成バインディング |
| --- | --- | --- |
| スカラーパス（`[]` なし） | `order.customer.city as city` | `let city = \|\| &order.customer.city;` |
| コレクション末尾（`path[]`） | `order.items[] as items` | `let items = \|\| order.items.iter();` |
| 射影コレクション（`path[].field`） | `order.items[].sku as sku` | `let sku = \|\| order.items.iter().map(\|n\| &n.sku);` |

すべてのエイリアスバインディングはクロージャ `|| ...` として生成する。呼び出しのたびに新しい参照・イテレータを生成するため、ボロウ競合および単一消費の問題を回避できる（§11.16 参照）。body 内では `name()` として呼び出す。

> **二層分離:** クロージャは Rust レベルの実装手段である。DDC の静的検証はコントラクト宣言（`Read: path as name`）を権威ソースとする。生成クロージャのランタイム評価は DDC 検証モデルの外にある。BodyAnalyzer はエイリアス mapping を `ReadPath.alias` から直接取得し、生成クロージャは参照しない。

コード生成エラー:

```rust
pub struct CodegenError {
    pub message: String,
}
```

依存クレート: `proc-macro2 = "1"`, `quote = "1"`

---

## 8. ビルドドライバー（`ddc-build`）

### 8.1 使用方法

DDC を使うクレートの `build.rs`:

```rust
fn main() {
    ddc_build::compile("src/");
}
```

生成されたファイルの取り込みは `src/lib.rs` 等で:

```rust
include!(concat!(env!("OUT_DIR"), "/ddc_generated.rs"));
```

### 8.2 `compile()` の処理フロー

```rust
pub fn compile<P: AsRef<Path>>(src_dir: P)
```

1. `src_dir` 以下の `*.ddc` ファイルを再帰的に走査する
2. `println!("cargo:rerun-if-changed=...")` で Cargo に変更監視を登録する
3. 各ファイルを `ddc_syntax::parse_ddc_file()` でパースして `DdcFile` を得る
4. 全 `DdcFile` を `ContractStore::from_files()` に渡して `ContractStore` を構築する
5. 実効契約の計算は `ContractStore::effective()` 呼び出し時に内部で実行される（Lazy LFP）
6. `validate_readonly()` / `validate_kernel_boundary()` で宣言契約レベルを検証する
7. `BodyAnalyzer::validate()` で body の DDC 適合性を検証する（§11.14 参照）
8. `ddc_codegen::codegen()` で `TokenStream` を生成して文字列化する
9. `$OUT_DIR/ddc_generated.rs` に書き出す

エラーが発生した場合、`eprintln!` でメッセージを出力して `panic!` でビルドを停止する。

### 8.3 実装ロードマップ

```
フェーズ 1  ddc-syntax パーサー（AST + DdcFnSig）
    ├── フェーズ 2  DependencyGraph 構築（Tarjan SCC）  ─┐
    │         └── フェーズ 3  LFP + readonly/kernel 検証  ┘ 並列開発可
    └── フェーズ 4  BodyAnalyzer（syn body 検証）           ┘
                           ↓ フェーズ 1〜4 完了後
                  フェーズ 5  ddc-codegen（TokenStream 生成）
```

| フェーズ | クレート | 主な成果物 | 前提フェーズ |
| --- | --- | --- | --- |
| 1: ddc-syntax パーサー | `ddc-syntax` | `DdcFile` AST、`DdcFnSig`、`Param`、`DeclaredContract` | なし |
| 2: DependencyGraph | `ddc-core` | `DependencyGraph`（ノード・エッジ・kernel フラグ） | 1 |
| 3: LFP + 検証 | `ddc-core` | `ContractStore::from_files`、`effective`、`validate_*` | 2 |
| 4: BodyAnalyzer | `ddc-build` | `BodyAnalyzer`（syn body 検証、エイリアス解決） | 1 のみ |
| 5: ddc-codegen | `ddc-codegen` | `TokenStream` 生成、alias クロージャ挿入 | 1〜4 |

> **フェーズ 2/3 と フェーズ 4 は並列開発可能。** BodyAnalyzer は `DeclaredContract`（フェーズ 1 で生成）のみを参照し、`DependencyGraph` には依存しない。

**パフォーマンス設計（フェーズ 3 実装時に適用）:**

| 戦略 | 内容 | 採用方針 |
| --- | --- | --- |
| 遅延評価（Lazy LFP） | `effective(id)` 初回呼び出しまで Tarjan+LFP を遅延。`OnceLock` でキャッシュ。kernel cut-point がグラフを自然分割するため計算対象が絞られやすい | **初期実装から導入** |
| rayon 並列化 | 独立 SCC を並列 LFP 計算。kernel 境界が多いほど効果大 | 大規模化時に追加 |
| cross-file エッジ分割 | ファイル内外エッジを分離管理し、変更波及のないサブグラフの再計算をスキップ | 将来課題 |

---

## 9. クレート間の依存関係まとめ

```toml
# ddc-syntax/Cargo.toml
[dependencies]
# 外部依存なし（pure テキスト処理）

# ddc-core/Cargo.toml
[dependencies]
ddc-syntax = { path = "../ddc-syntax" }

# ddc-codegen/Cargo.toml
[dependencies]
ddc-syntax  = { path = "../ddc-syntax" }
proc-macro2 = "1"
quote       = "1"

# ddc-build/Cargo.toml
[dependencies]
ddc-syntax  = { path = "../ddc-syntax" }
ddc-core    = { path = "../ddc-core" }
ddc-codegen = { path = "../ddc-codegen" }
syn         = { version = "2", features = ["full", "visit"] }
```

`ddc-syntax` は proc-macro2 に **依存しない**。これは `.ddc` がテキストベースのスタンドアロンファイルであることを示す設計上の決定である。
`proc-macro2` / `quote` は Rust コードを生成する `ddc-codegen` のみが依存する。

---

## 10. 廃止された設計

以下は過去に検討・実装されたが廃止された。

| 廃止要素 | 理由 |
| --- | --- |
| `ddc-macro` クレート（proc-macro） | DDC はスタンドアロン `.ddc` ファイルとして記述する。Rust の `ddc!{}` マクロとして `.rs` ファイルに埋め込む設計は根本的に誤りだった |
| `DdcFunction.body_tokens: TokenStream` | `TokenStream` は proc-macro2 依存を強制する。Rust 生 body は文字列 (`DdcBody::Rust(String)`) として保持する |
| `parse_ddc_function(TokenStream)` | proc-macro 入力前提の API。`parse_ddc_file(&str)` に置き換え |
| `ContractStore::register(DdcFunction)` | 旧型名。`DdcFn` に統一 |
| `scan::build_contract_store()` による `.rs` 走査 | `.rs` ファイル内 `ddc!` ブロック探索は廃止。`.ddc` ファイル走査に変更 |
| `ddc-build::check()` API | `compile()` に改名し、Rust コード生成まで担う |
| `fn` キーワードによる関数宣言 | C# スタイル（戻り型先頭）に統一。`fn` キーワード、`->` 記法、`name: Type` 引数順は使用しない |
| `Option<T>` / `Vec<T>` / `()` | DDC 構文では `Optional<T>` / `List<T>` / `void` を使用。codegen で Rust 型名に変換する |
| `Result<T, E>` のシグネチャ利用 | DDC シグネチャは依存契約（Read / Write / Call）と直交させるため、`Result<T, E>` は関数シグネチャに現れない |
| `realize` キーワード | `class` に統一（§11.7 参照） |
| `impl Type: Contract` 記法 | `class Type : Contract` に統一（§11.7 参照） |
| `IRepository::load`（`::` 区切り） | `IRepository.load`（`.` 区切り）に統一（§11.9 参照） |
| `[IRepository.load]` アノテーション方式 | `:` サフィックス方式に決定（§11.10 参照） |
| `DdcBody::Ddc(Vec<DdcStmt>)` | DDC 独自式言語の body バリアント。body は Rust 構文に統一（§11.14 参照） |
| エイリアス区切り記法 `path :: name` | `::` は §11.9 で廃止済みの記法と同記号のため不採用。`as` キーワード（`path as name`）に統一（§11.16 参照） |
| `ContractStore::new()` / `register(&mut self, DdcFn)` / `compute_effective_contracts(&mut self)` | 可変状態 API は廃止。`ContractStore::from_files()` に統一（§6.3 参照） |
| `sig: String`（`DdcFn` / `ContractMethod` / `ImplMethod`） | `DdcFnSig` 構造体に置き換え（§4.5、§11.13 参照） |

---

## 11. 構文設計の決定記録

本節は DDC の構文設計において議論・決定された事項の記録である。
後続の実装者・設計者が同じ迷いを繰り返さないための根拠として保持する。

### 11.1 対象読者と構文方針

**決定:** DDC の構文は C# / Java スタイルに統一する。

**根拠:**
- DDC が価値を発揮する業務システム・高信頼性システムの文脈では、日本を含め Java / C# が主流言語である
- Rust スタイル（`fn`、`->`、`name: Type`）は Rust 開発者には自然だが、業務系開発者の学習コストが高い
- C# スタイルは業務系開発者が最も馴染みやすい構文であり、迎合ではなく合理的な設計判断である

### 11.2 `fn` キーワードの廃止

**決定:** DDC 関数宣言に `fn` キーワードを使用しない。

**根拠:**
- `fn` は Rust 固有のキーワード。C# / Java 開発者には違和感がある
- C# / Java スタイルでは戻り型が先頭に来るため `fn` が構文上不要になる
- 廃止された構文: `fn process_shipping(order: &mut Order) -> Result<(), ShippingError> ( ... ) { ... }`

### 11.3 `void` の採用

**決定:** 戻り値なしは `void`。`()` は使用しない。

**根拠:**
- `()` は Rust の unit 型。C# / Java 開発者には馴染みがなく、関数宣言の先頭に来た場合に「型名」として認識されない
- `void` は C# / Java 双方で標準的な「戻り値なし」の表現である

### 11.4 `Optional<T>` の採用

**決定:** 省略可能型は `Optional<T>`。`Option<T>` も `?` も使用しない。

**根拠:**
- `Option<T>` は Rust 型名。C# / Java 開発者には馴染みがない
- `?` は nullable 記法だが、`?:` 三項演算子・`?.` null 条件演算子・`??` null 合体演算子など意味が文脈依存で不安定
- Java の `Optional<T>` が業務系開発者に最も馴染みやすく、意味が明確

### 11.5 `List<T>` の採用

**決定:** コレクション型は `List<T>`。`Vec<T>` は使用しない。

**根拠:**
- `Vec<T>` は Rust 固有の型名
- C# / Java 双方で `List<T>` が標準的なシーケンス型として認識されている

### 11.6 `Result<T, E>` の廃止

**決定:** DDC の関数シグネチャに `Result<T, E>` を使用しない。

**根拠:**
- `Result<T, E>` は Rust の「例外を使わない」設計に由来する型
- DDC は依存契約（Read/Write/Call）を記述する言語であり、失敗経路（throw/panic）は制御流として契約外に置く
- エラー関連の依存は `Call: ::Exception.Constructor()` / `Write: ::Exception.Message` で表現できる

```
// 廃止（Rust スタイル）
fn process(order: Order) -> Result<(), ShippingError> { ... }

// 採用（C# スタイル）
void process(Order order) (
    Call: ::Exception.Constructor(),
    Write: ::Exception.Message
) { ... }
```

### 11.7 `class` キーワードの採用

**決定:** contract 実装宣言には `class Type : Contract` を使用する。

**経緯（迷いの記録）:**
1. **`impl Type : Contract`** — Rust の `impl` キーワードをそのまま使用。Rust の `impl Trait for Type` とは語順・記号が異なるため Rust 開発者が混乱する。`SqlRepository : IRepository` が型境界（trait bound）に見えるリスクがある
2. **`realize Type : Contract`** — 言語中立で意味が明確（「型がコントラクトを実現する」）。しかし業務系開発者には馴染みのないキーワードである
3. **`class Type : Contract`** — C# の class 宣言と完全に同形。業務系開発者が毎日書く構文。**採用**

**根拠:**
- `class SqlRepository : IRepository` は C# 開発者が「SqlRepository は IRepository を実装するクラス」と即座に読める
- `:` は C# / Java における「継承・実装」の標準的な区切り記法
- `class` キーワードが「contract を実現する型の宣言」であることが直感的に伝わる

### 11.8 `contract` キーワードの維持

**決定:** インターフェース相当宣言には `contract` キーワードを使用する。`interface` は使用しない。

**根拠:**
- C# において `interface` という語は I/O（入出力）の概念を連想させる側面がある（`IDisposable`、`IEnumerable` など、多くが何らかの操作を抽象化したもの）
- DDC の意図は「依存の宣言契約」であり、`contract` の方が意味を正確に表現する
- `IRepository` のような `I` プレフィックスの命名慣習はそのまま利用できる

### 11.9 `.` 区切りの採用

**決定:** contract メソッド参照のパス区切りは `.`（例: `IRepository.load`）。`::` は使用しない。

**根拠:**
- `::` は Rust のパス区切り。C# / Java 開発者には馴染みがない
- C# / Java では `.` がメンバーアクセスの標準記法
- `IRepository.load` は C# の明示的インターフェース実装（`IFoo.Method()`）に近い
- 廃止された記法: `fn load() -> T : IRepository::load { ... }`

### 11.10 `:` サフィックス方式の採用（アノテーション方式の不採用）

**決定:** class 内メソッドの契約委譲は `: ContractName.MethodName` サフィックスで表現する。

**検討されたアノテーション方式:**
```
[IRepository.load]
Optional<User> load(UserId id) { ... }
```

**不採用の理由:**
- アノテーションは構文上「付加的・省略可能」に見える（Java の `@Override` が省略可能なのと同様の印象）
- アノテーションがない場合の扱い（追加メソッドとして許可するか、エラーにするか）の仕様判断が生じる
- 「追加メソッドとして許可」にした場合、契約参照の漏れが静かに通過する危険がある
- `:` サフィックスはメソッドシグネチャ行の一部であるため、**構文レベルで必須**にできる
- 契約参照の漏れがパースエラーとして即座に検出される

### 11.11 アクセス修飾子の許容

**決定:** `public` / `private` 等のアクセス修飾子を DDC 関数に付与できる。

**根拠:**
- 公開範囲（`public` / `private`）と DDC 契約制約（`Read:` / `Write:` 等）は**直交する概念**
- 公開範囲はモジュール設計の問題、DDC 契約は依存境界の問題であり、それぞれ独立して宣言できることが設計意図と一致する

### 11.12 `[kernel]` アノテーションの維持

**決定:** OS レベルの副作用境界を示すアノテーションは `[kernel]` とする。

**根拠:**
- OS kernel の概念（ユーザー空間から分離された、DDC 検証が不可能なシステムコール境界）と意味が一致する
- 代替名（`[native]`, `[unsafe]`, `[extern]` 等）も検討したが、`[kernel]` が最も正確に意図を表す
- `[kernel]` 関数の境界は DDC 検証スコープの外であり、OS kernel の「検証不可能な実装レイヤー」と同じ意味を持つ

### 11.14 関数 body の Rust 構文採用と制約

**決定:** DDC 関数の body は Rust 構文で記述する。DDC 独自の式言語は実装しない。

**根拠:**
- DDC は「Rust に射影される言語」であり、body は最終的に Rust コードとして emit される
- DDC 独自式言語を設計・実装するコストは高く、独自言語の学習コストを新たに課すだけである
- DDC の本質的価値は宣言部（`( Read: ... Write: ... )`）の契約検証にあり、body の構文は副次的
- 宣言と body で型名の書き方が統一されていることが、一人の開発者が `.ddc` ファイルを一貫して記述するうえで重要

**body 内の構文制約:**

| 制約 | 内容 |
| --- | --- |
| `unsafe` 禁止 | 通常の DDC 関数では `unsafe` ブロック使用不可。`[kernel]` 関数のみ許可 |
| `panic!` / `unwrap()` / `expect()` 禁止 | 依存契約で表現できない暗黙の失敗経路を抑止する |
| `Optional<T>` / `List<T>` | body 内でも使用できる。codegen が生成ファイル冒頭に `pub type Optional<T> = Option<T>;` / `pub type List<T> = Vec<T>;` を emit するため有効。`void` は return 型宣言専用（body 内の型式には出現しない） |
| `?` 演算子 | 使用可。条件は明示的に 2 つのみ: (1) `?` を適用する呼び出し元は `Call:` に宣言されていること、(2) `?` の前後で実行されるフィールド read/write が `Read:` / `Write:` に宣言されていること。未宣言なら `BodyUndeclaredCall` / `BodyUndeclaredRead` / `BodyUndeclaredWrite` としてエラー |

**宣言と body の型名統一:**

```
// 宣言・body ともに C# スタイル型名に統一して記述できる
public Optional<User> find(UserId id)
(
    Read: id.value
    Call: self.db.lookup()
) {
    let result: Optional<User> = self.db.lookup(id.0)?;
    Ok(result)
}
```

codegen が生成ファイル冒頭で `pub type Optional<T> = Option<T>;` / `pub type List<T> = Vec<T>;` を emit するため、body 内の `Optional<T>` / `List<T>` は Rust コンパイラにそのまま認識される。Rust ネイティブの `Option<T>` / `Vec<T>` も引き続き有効であり、どちらで書いてもよい。

**body の DDC 適合性検証:**

DDC の核心保証は、宣言契約の整合性チェックのみでは成立しない。body が宣言を無視して自由に実装できる場合、DDC の契約は設計ドキュメントに過ぎなくなる。適合 Rust 実装は、`syn` クレートを使って body テキストを構文解析し、body 内の実際のアクセスが宣言契約と一致することを検証しなければならない。

`ddc-build` 内の `BodyAnalyzer` は `syn::visit::Visit` トレイトを実装し、`syn::parse_str::<syn::Block>()` でパースした body AST を走査して以下を検証する:

| 検証項目 | 検証内容 |
| --- | --- |
| フィールド読み取り | body 内の全フィールドアクセス式を canonical path に解決し、`Read:` または `Write:` 宣言と照合する |
| フィールド書き込み | body 内の全フィールド代入式を canonical path に解決し、`Write:` 宣言と照合する |
| 関数呼び出し | body 内の全呼び出し式を `Call:` 宣言と照合する |
| unsafe ブロック | `[kernel]` アノテーションを持たない関数の body に `unsafe` ブロックが存在する場合エラー |
| 禁止マクロ | `panic!` / `unwrap()` / `expect()` の存在をエラーとする |

検証エラーは `ValidationError` として返し、`ValidationErrorKind::Body*` バリアントを使用する（§6.4 参照）。

**エイリアス解析:**

DDC の契約宣言はエイリアス不変である。エイリアスの宣言は `Read:` セクションの `as identifier` **コントラクトレベル宣言のみ**で行う。body 内の `let` バインディングはエイリアス宣言として扱わない（§11.16 参照）。

`BodyAnalyzer` は `ReadPath.alias` から直接エイリアス mapping を取得する。推論処理は不要。

```
// DDC 契約
Read:
    order.items[].sku as sku

// BodyAnalyzer の mapping（ReadPath.alias から直接取得）
sku → order.items[].sku
```

body 内で別名経由のアクセスを行いたい場合は、必ず `Read: path as name` をコントラクトに宣言する。コントラクト宣言なしの `let` バインディングはエイリアスとして解決されない。

```rust
// NG: コントラクト宣言なしの let バインディングによる別名アクセス
let items = &order.items;          // items はエイリアスとして認識されない
for item in items { item.sku }     // items が未解決のためエラー

// OK: コントラクトエイリアスを宣言して使用
// Read: order.items[] as items → codegen が let items = || order.items.iter(); を挿入
for item in items() { item.sku }   // items → order.items[] として解決済み

// OK: パスを直接使用
for item in &order.items { item.sku }  // order.items を直接参照
```

このエイリアス不変性が、DDC 契約が「どのように実装されているか（構文形式）」ではなく「何に依存しているか（意味論）」を宣言するという設計原則の Rust 実装における核心である。

---

### 11.13 `DdcFnSig` — シグネチャの構造化

**決定:** `DdcFn.sig`（および `ContractMethod.sig`、`ImplMethod.sig`）を `DdcFnSig` 構造体として保持する（`sig: String` は廃止）。

**設計（§4.5 参照）:**
- `DdcFnSig { return_type: String, name: FunctionId, params: Vec<Param> }` でシグネチャを分解する
- `Param { type_name: String, param_name: String }` で各パラメータを保持する
- パーサーは C# スタイルのテキスト（例: `"Optional<User> load(UserId id)"`）をこの型に分解する

**codegen の責務:**
- `return_type` の変換: `Optional<T>` → `Option<T>`、`List<T>` → `Vec<T>`、`void` → `()`
- `params` の変換: C# スタイル（`Type name`）→ Rust スタイル（`name: Type`）の引数順反転
- 変換後を Rust スタイル（例: `fn load(id: UserId) -> Option<User>`）で emit する

**ソースマップの必要性（未解決課題）:**
- C# スタイル DDC と生成 Rust コードは見た目が大きく異なるため、rustc のエラーメッセージが `.ddc` の行番号に対応づかない問題が将来的に発生する
- 解決策は `ddc-codegen` でのソースマップ生成（生成コードへの行番号情報の埋め込み）
- これは現時点での未実装課題として認識されている

---

### 11.15 DDC が Rust 型システムに加えて提供する価値

本節は、DDC の Rust 実装が Rust の型システムだけでは達成できない価値を整理する。Rust の所有権・借用モデルは DDC の設計と競合しない。DDC は Rust の型安全性を前提として、その上位に「依存契約」の層を重ねる。

**1. エイリアス不変な依存宣言**

Rust の型システムは `Vec<Item>` を受け取った関数が `item.sku` のみにアクセスするか全フィールドにアクセスするかを区別しない。DDC の契約パスは `order.items[].sku` という意味論的表現であり、body の構文形式（ローカル変数・イテレータ変数・メソッドチェーン等）に依存しない。エイリアスを通じたアクセスも canonical path に解決された後に宣言と照合されるため、リファクタリングによる body の書き換えは契約に影響しない。

**2. 推移的副作用の静的可視化**

Rust の関数シグネチャは呼び出し先が内部で何を変更するかを型として記述しない（`Rc<RefCell<T>>` 等の内部可変性を通じた変更は `&self` シグネチャから不可視）。DDC の実効契約は `Call:` チェーン全体の Read/Write/Call を代数的に展開し、呼び出し元から推移的副作用を静的に読み取れる。あるパス `p` を変更する関数を追跡するために実行時解析は不要であり、実効契約のグラフ走査で完結する。

**3. `readonly` の推移的契約保証**

Rust の `&self` は直接的な変更を防ぐが、`Cell<T>` / `RefCell<T>` / `Mutex<T>` 等を通じた内部可変性は `&self` シグネチャで検出できない。DDC の `readonly` 修飾子は `Call:` チェーン全体に推移的に適用され、`readonly f` が呼び出せる関数は `readonly` または `[kernel]` に限定されることが契約レベルで強制される。この保証は Rust の型システムではなく DDC の検証パスによって提供される。

**4. `[kernel]` による検証境界の形式的分離**

Rust の `unsafe` ブロックはコンパイラの借用検査を局所的に無効化するが、その副作用範囲を外部に宣言する仕組みがない。DDC の `[kernel]` アノテーションは OS・ハードウェア境界の副作用を依存契約の外へ隔離し、Read/Write 効果が呼び出し元に伝播しないことを形式的に保証する。`[kernel]` 境界の外側では通常の DDC 検証が有効であり、「検証可能な領域」と「検証スコープ外の領域」の境界が明示される。

**5. コントラクトによる多態性と capability 保証**

Rust の trait は「このメソッドが存在する」という構造的契約を記述するが、「このメソッドがどの副作用を持つか」を記述しない。DDC の `contract` は capability envelope（§2.5 参照）を宣言し、具象実装がその envelope を超えた依存を持てないことが検証される。これにより、コントラクト越しの呼び出しにおいて呼び出し元が「最大でこれだけの副作用が起きうる」と静的に知ることができ、具象実装の差し替えに対してそれが変化しないことが保証される。

---

### 11.16 `Read:` パスエイリアス宣言（`as` 修飾子）

**決定:** `Read:` セクションに `as identifier` 修飾子を付与してコントラクトレベルのエイリアスを宣言できるようにする。エイリアスはクロージャとして codegen に展開される。

**根拠:**

**1. BodyAnalyzer の推論複雑度の解消**

body 内の `let` バインディングからパスを逆引きするエイリアス推論は複雑なデータフロー解析を要し、一般ケースでは NP 困難に近い問題となる。コントラクトレベルでエイリアスを宣言することで、BodyAnalyzer はエイリアス名とパスの対応を `ReadPath.alias` から直接取得でき、推論処理が不要になる。

**2. 依存空間の名前付け（射影ではない）**

エイリアスの本質的意味は「依存可能空間への名前付け」である。`Read: order.items[].sku as sku` は「`order.items[].sku` という依存空間（`order.items.select(n -> n.sku)` に相当）を body 内では `sku` と呼ぶ」という宣言であり、その空間が具体的に何を計算するかを記述するものではない。これは Pick 系型射影や LINQ Select（値の変換・抽出）とは区別される。

**3. クロージャ生成の選択理由**

生成バインディングを `let x = &path;`（参照保持）ではなくクロージャ `let x = || ...;` とした理由:

- **参照依存の排除**: `let x = &something;` は関数スコープ全体でその対象の不変借用を保持する。`Write:` パスへの後続操作とボロウチェッカー競合が生じうる
- **都度生成**: クロージャは呼び出しのたびに新しい参照・イテレータを生成するため、複数回使用や条件分岐内での使用が安全
- **`Read:` 専用**: `Write:` パスは `&mut` 参照が必要なケースが多く、自動生成のリスクが高い。`Write:` エイリアスは現仕様に含まれない

**4. `as` キーワードの選択理由**

- Rust の `use Foo as Bar` / C# の `using Foo = Bar` と整合する「別名付け」の慣用的キーワード
- `::` は §11.9 で廃止済みの記法であり再使用しない
- `:=` は代入式との混同を招く

**5. body 内エイリアス推論の禁止**

body 内の `let x = &something;` バインディングをエイリアスとして推論するアプローチは採用しない。

- データフロー解析の複雑度が高く、一般ケースでは追跡困難な問題となる
- 推論結果が実装依存となり、適合実装間で検証結果が異なる可能性がある
- コントラクト文書を読むだけでは依存空間の命名が把握できず、可視性が低下する
- コントラクトレベルエイリアスが利用可能である以上、推論の実用的価値はない

エイリアスは必ずコントラクトに `Read: path as name` として宣言する。body 内の命名は常にコントラクトが権威的ソースとなる。

**生成パターンまとめ:**

| パスの形式 | 宣言例 | 生成バインディング | body 内での呼び出し |
| --- | --- | --- | --- |
| スカラーパス | `order.customer.city as city` | `let city = \|\| &order.customer.city;` | `city()` |
| コレクション末尾 | `order.items[] as items` | `let items = \|\| order.items.iter();` | `items()` |
| 射影コレクション | `order.items[].sku as sku` | `let sku = \|\| order.items.iter().map(\|n\| &n.sku);` | `sku()` |

---

### 11.17 class 内メソッドの契約形式 — 委譲と独自宣言の選択

**決定:** class メソッドは `: ContractRef`（委譲形式）と `( ... )`（独自契約形式）のどちらかを選択できる。

**迷いの記録:**

「設計として最も整合的なのは、class 内のすべてのメソッドの契約を `contract` ブロックとして宣言させることではないか」という議論があった。すべての依存契約が `contract` として外部から参照可能になり、capability envelope によって保護されるという意味で理想的に見える。

しかし、この方針を徹底すると private helper メソッドに対しても `contract` ブロックの宣言を強制することになる:

```
// 「全部 contract に書く」方針に従うと…
contract ISqlRepositoryHelper ( Read: id.value ) {
    void helper(UserId id) ( Read: id.value );
}

class SqlRepository : IRepository {
    public Optional<User> load(UserId id) : IRepository.load { ... }
    private void helper(UserId id) : ISqlRepositoryHelper.helper { ... }
}
```

**問題点:**
- 単一実装・単一箇所でしか使われない private helper に `contract` を書かせることは、`contract` キーワードの本来の意味（「複数実装を想定した公開インターフェース」）を希薄化させる
- private メソッドの依存宣言のためだけに `contract` ファイルが増殖し、実務上の負担が大きい
- `contract` を読む側から見ても、「これが実装クラス 1 つだけを対象にした宣言である」という情報が失われる

**解決:**

| 記述形式 | 意味 | `contract` の位置づけ |
| --- | --- | --- |
| class メソッド `: ContractRef` | contract を実装する。複数実装が想定される | 公開インターフェース仕様 |
| class メソッド `( ... )` | 実装内部の依存宣言。この class にしか存在しない | 不要（インラインで十分） |

`( ... )` を直接書くことは「この契約はここでしか使わない」という意図を構文レベルで表現する。`contract` ブロックは「複数の class に実装される可能性がある公開インターフェース」にのみ使用するという区別が自然に成立する。

**実装上の対応:**
- `ImplMethodKind` enum で `ContractImpl` と `OwnContract` を排他的に表現（`Option` 2フィールドより型レベルで不変条件を保証できる）
- `DependencyGraph` では `OwnContract` メソッドのみ修飾名（`TypeId.method_id`）でノード登録。`ContractImpl` は `contract_refs` 先の `ContractMethod` が既にノードを保持しているためスキップする

---

### 11.18 複数 contract の実装 — C# 暗黙的実装との対比

**背景:**

`class Foo : IBar, IBaz` のように複数の contract を実装する場合、`IBar` と `IBaz` の両方に同名メソッド（例: `read`）が存在することがある。

**C# での挙動:**

| C# の形式 | 動作 |
| --- | --- |
| 暗黙的実装 | 1 つのメソッド実装が同シグネチャを持つすべての IF を満たす |
| 明示的実装（`void IFoo.Do()`） | 同名メソッドを複数の独立した関数として実装できる |

**DDC / Rust での対応:**

Rust はメソッドオーバーロードも明示的インターフェース実装も持たない。同名メソッドは **1 つの Rust 関数** としてコンパイルされる。したがって DDC は C# の暗黙的実装スタイルのみを採用する。

- **C# 明示的実装（同名メソッドを複数関数）→ DDC では非対応**
- **C# 暗黙的実装（1 関数で複数 IF を満たす）→ DDC は `contract_refs: Vec<FunctionId>` で表現**

例:

```
class DualReader : IReader, ICache {
    public Data read(FileId id) : IReader.read, ICache.read { ... }
}
```

この 1 メソッドが `IReader.read` と `ICache.read` の両方を満たすことを宣言する。

**Phase 3 検証ルール（completeness check）:**
- `ImplDecl.contract_ids` 内の各 `ContractDecl.methods` の全メソッドが、いずれかの `ImplMethod.contract_refs` に含まれていること
- 同名 contract メソッドが複数 contract に存在し、1 つの impl メソッドが両方を `contract_refs` に列挙する場合、両 contract メソッドのシグネチャが同一であることを検証する（互換性なければ `ValidationError`）
- 1 つの `contract_ref` が複数の `ImplMethod` に登録されている場合はエラー（重複実装）

**実装上の変更（`contract_ref` → `contract_refs`）:**
- `ImplMethodKind::ContractImpl { contract_refs: Vec<FunctionId> }` — `Vec` の長さは 1 以上。空はパースエラー
- パーサは `: QualIdent (',' QualIdent)*` を読み、終端は `{`。コンマの曖昧性なし（contract 宣言ブロックの `,` とは位置が異なる）

---

### 11.19 `UndeclaredPathAccess` / `UndeclaredCall` / `UndeclaredThrow` 削除の根拠

**削除対象（`ValidationErrorKind` から除去）:**

```rust
UndeclaredPathAccess,  // ← 削除
UndeclaredCall,        // ← 削除
UndeclaredThrow,       // ← 削除（Throw セクション廃止に伴う）
```

**理由1: body レベルの検証は `BodyUndeclared*` が担当**

Tech.md §5.5 / §7.2 / §8 で定義される「Undeclared」系チェックは body テキスト解析に基づく。つまり「body が `Call:` に宣言されていない関数を呼び出している」という検証である。これは `BodyUndeclaredCall` として Phase 4 (BodyAnalyzer) が担当する。

ContractStore レベルに `UndeclaredCall` を置くと「宣言契約 vs 実効契約」の整合性チェックという別の意味になるが、それは DDC の設計意図とずれる。

**理由2: ContractStore レベルでの envelope excess は DDC/Rust 構造的に不可能**

C# DDC では `ContractDecl` に `envelope`（メソッドの合算上限契約）が存在し、個々の `ContractMethod` が `envelope` を超えていないかの検証（envelope excess check）が意味を持つ可能性がある。

しかし Rust DDC では:
- `ContractImpl` は独立した `DeclaredContract` を持たない — `contract_refs` で既存の `ContractMethod` を参照するだけ
- `ImplMethod(ContractImpl)` はグラフノードに登録されない（契約は参照先 `ContractMethod` が保持）
- したがって "ContractStore が把握できない宣言" は存在せず、envelope excess 相当のチェックは実施不可能

**結論:**

| バリアント | 削除理由 |
|-----------|---------|
| `UndeclaredPathAccess` | body レベル → `BodyUndeclaredRead` / `BodyUndeclaredWrite` が担当 |
| `UndeclaredCall` | body レベル → `BodyUndeclaredCall` が担当 |
| `UndeclaredThrow` | Throw セクション廃止に伴い仕様対象外 |

ContractStore が担う宣言契約レベルの検証は `ReadonlyViolation`（effective.Write 非空）と `KernelBoundaryViolation`（[kernel] に Read:/Write:/Call: 宣言）の2つのみ。
