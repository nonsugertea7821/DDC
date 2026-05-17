# Declarative Dependency Control (DDC) 言語仕様書

---

## 1. 概要

### 1.1 文書の目的と適用範囲

本文書は Declarative Dependency Control（以下 DDC）の言語仕様を定義する。DDC は、関数が依存する構造パスを静的契約として宣言し、未宣言依存へのアクセスを禁止するモデルである。

**本仕様が定義する対象:**

- 契約宣言の構文と文法規則
- `Read` / `Write` / `Call` / `Throw` 各セクションの意味論
- 構造パスの文法および評価規則
- 宣言契約・実効契約の合成規則
- `readonly` 関数の制約
- `[kernel]` アノテーションと検証スコープ境界
- Dependency Closure の要件
- 適合実装が報告しなければならないエラー条件

**本仕様が定義しない対象:**

- DDC コンパイラ・静的解析ツールの実装アルゴリズム
- ホスト言語（C#、Java、TypeScript 等）への具体的な統合手順
- コード生成・実行モデル
- エラーメッセージの書式

### 1.2 文書の規約

本文書では、要件レベルを以下のように区別する。

| 表現 | 意味 |
| --- | --- |
| **しなければならない** | 適合実装が遵守すべき規範要件 |
| **してはならない** | 適合実装が禁止すべき規範要件 |
| **するべきである** | 推奨されるが必須ではない |
| **してよい** | 実装の裁量に委ねる |

コードブロック中の `// NG:` は違反例、`// ok:` は正当な記述例を示す。

### 1.3 DDC が解決する問題

既存のオブジェクト指向言語では、オブジェクト参照を取得した時点でその内部構造全体へ到達可能となる。

```csharp
void ProcessShipping(Order order)
```

この関数は `Order` を受け取った瞬間、理論上は `order.Customer.Address.Country.Currency` まで依存可能となる。しかし配送処理が実際に必要とするのは `order.Id` や `order.Items[].SKU` のみかもしれない。既存の型システムはこの「どこまで依存しているか」を表現できない。

これにより以下の問題が発生する:

- **変更影響範囲の不透明化**: 構造変更がどの関数に影響するかを静的に特定できない
- **DTO・ViewModel の増殖**: 依存を局所化するための追加抽象化が必要になる
- **カプセル化崩壊**: 参照さえ入手すれば任意の内部構造にアクセス可能になる
- **暗黙的依存の不可視性**: 実際の依存範囲は実行時解析なしに把握できない

大規模コードベースでは、依存境界の推論コストはすでに発生している。しかしそれは人間の頭の中で、非形式的に、検証不可能な形で支払われている。DDC はこのコストをツール側へ移転し、静的に検証可能な形で扱うことを目指す。

### 1.4 DDC の立場

DDC は「構造の存在（Existence）」と「構造への依存権（Dependency Right）」を分離する。

```text
構造が public に存在する ≠ その構造へ依存してよい
```

関数が依存する構造パスは、`Read` / `Write` / `Call` / `Throw` として契約に静的宣言されなければならない。未宣言パスへのアクセスはコンパイル・静的解析段階で禁止される。

これは従来の可視性制御（`public` / `private` / `protected`）とは異なる制御レベルである。可視性は「アクセス先が持つ属性」であるのに対し、DDC 契約は「アクセス元が持つ権限」を制御する。

```text
従来: visible → accessible → dependency-possible（三者同一）
DDC:  visible ≠ dependency-possible（可視性と依存権の分離）
```

### 1.5 型システムとの違い

`TypeScript の Pick<Order, 'id' | 'items'>` のように、依存を型で制限するアプローチとの違いは次の通りである。

| 観点 | Pick 系アプローチ | DDC |
| --- | --- | --- |
| 強制 | 任意の慣習 （実装者が無視できる） | コンパイラが強制　|
| Read/Write/Call の区別 | できない | 独立して制御できる |
| ドメイン型の維持 | 派生型が増殖（DTO 爆発） | 型を分割しない。`Order order` のままドメイン意味を保つ |
| 契約の伝播 | なし | `Call:` による代数的統合 |
| グラニュラリティ | フィールド単位 | 構造パス単位（ネストしたトラバーサルを表現できる） |

型は「値の形」を記述し、DDC の契約は「アクセス行為の権限」を記述する。この二つは直交する概念である。

### 1.6 用語定義

| 用語 | 定義 |
| --- | --- |
| **構造パス（structure path）** | オブジェクトグラフ上のアクセス経路を示す式（例: `order.Items[].SKU`） |
| **宣言契約（declared contract）** | ある関数の契約ブロックに明示的に記述された依存の集合 |
| **実効契約（effective contract）** | 宣言契約と、`Call:` 宣言による呼び出し先の実効契約の和集合 |
| **capability** | ある関数が保持する、特定の構造パスへのアクセス権限 |
| **capability envelope** | ある関数の宣言契約が定義する、依存してよい上限の集合 |
| **依存権（dependency right）** | 契約に宣言されたアクセスを行う権限。構造の存在（可視性）とは独立 |
| **ホスト言語（host language）** | DDC 契約が付与される対象の言語（C#、Java、TypeScript など） |
| **直接作用（direct effect）** | 呼び出し先に委譲せず、関数実装自身が直接行う操作 |
| **例外生成点（throw site）** | `throw` 文が存在する関数フレーム |
| **Call Reachability** | `Call:` チェーンを通じた推移的な到達可能性 |
| **`[kernel]` アノテーション** | 関数を DDC 検証スコープ境界として指定するアノテーション。内部の Read/Write は DDC 検証対象外となる |
| **検証スコープ境界（verification scope boundary）** | `[kernel]` が示す境界。この境界の外側（呼び出し元）へは `effective.Read` / `effective.Write` が伝播しない |

---

## 2. 基本構文

### 2.1 契約宣言の文法

DDC の契約はホスト言語の関数宣言に付随するブロックとして記述される。EBNF による文法規則は以下の通りである。

```ebnf
contract-block    ::= "(" contract-sections ")"
contract-sections ::= read-section? write-section? call-section? throw-section?

read-section      ::= "Read:" NEWLINE read-entry+
write-section     ::= "Write:" NEWLINE path-entry+
call-section      ::= "Call:" NEWLINE call-entry+
throw-section     ::= "Throw:" NEWLINE type-entry+

read-entry        ::= INDENT path ("as" identifier)? NEWLINE
path-entry        ::= INDENT path NEWLINE
call-entry        ::= INDENT call-path "()" NEWLINE
type-entry        ::= INDENT type-name NEWLINE

path              ::= root-identifier path-segment*
path-segment      ::= "." identifier | "[]" | "[]" "." identifier
call-path         ::= identifier ("." identifier)*
root-identifier   ::= identifier
type-name         ::= identifier ("." identifier)*
identifier        ::= ALPHA (ALPHA | DIGIT | "_")*
annotation        ::= "[" annotation-name "]"
annotation-name   ::= "kernel"

contract-declaration   ::= "contract" identifier "(" contract-sections ")" "{" contract-method* "}"
contract-method        ::= type-name identifier "(" param-list? ")" contract-block
impl-declaration       ::= "class" identifier ":" identifier "{" impl-method* "}"
impl-method            ::= type-name identifier "(" param-list? ")" ":" call-path
param-list             ::= param ("," param)*
param                  ::= type-name identifier
```

各セクションは省略可能である（§2.4 参照）。セクションの順序は固定であり、`Read:` → `Write:` → `Call:` → `Throw:` の順に記述しなければならない。

### 2.2 関数宣言との結合

DDC 契約は、ホスト言語の関数宣言（メソッド宣言）の直後、本体（`{}`）の前に置かれる。

```ddc
[修飾子] [戻り型] [メソッド名]([引数リスト])
(
    Read:
        [パスリスト]
    Write:
        [パスリスト]
    Call:
        [呼び出しリスト]
    Throw:
        [例外型リスト]
);
```

`readonly` 修飾子（§9 参照）は関数修飾子の位置に記述する。

```ddc
public readonly User Find(UserId id)
(
    Read:
        id.Value
    Call:
        repository.Load()
);
```

`[kernel]` アノテーション（§12.6 参照）は関数宣言の直前の行に置かれる。`[kernel]` 関数の契約ブロックは `Throw:` セクションのみを含んでよい。

```ddc
[kernel]
public void Send(byte[] bytes)
(
    Throw:
        NetworkDisconnectedException
);
```

### 2.3 完全な例

```ddc
public void ProcessShipping(Order order)
(
    Read:
        order.Id
        order.Items[].SKU
    Write:
        order.Status
    Call:
        outerRepository.GetSomeData()
    Throw:
        IOException
);
```

### 2.4 契約ブロックの必須要件とセクションの省略規則

**契約ブロック自体は省略できない。** DDC 関数宣言には必ず契約ブロック `()` を記述しなければならない。契約ブロックを省略した関数宣言は適合実装がコンパイルエラーとしなければならない。

契約ブロック内の各セクションは省略可能である。省略されたセクションは空の集合と等価である。

| 省略されたセクション | 意味 |
| --- | --- |
| `Read:` | 関数はいかなる構造パスも直接読み取らない |
| `Write:` | 関数はいかなる構造パスも直接変更または構築しない |
| `Call:` | 関数はいかなる他の関数も呼び出さない |
| `Throw:` | 関数は例外生成点を持たない |

すべてのセクションが省略された空の契約ブロック `()` は、副作用・依存がまったく存在しない関数 `noop` を意味する。

```ddc
public void noop()(){ return; };
```

### 2.5 コントラクト宣言

コントラクト型のメソッドにも契約を宣言できる。コントラクトは、具象実装に対するアクセス権限の上限（capability envelope）をコントラクトレベルとメソッドレベルの二層で定義する。

```ddc
contract IRepository
(
    Read:
        id.Value
)
{
    User Load(UserId id)
    (
        Read:
            id.Value
    );
}
```

外側の `()` はコントラクト全体の capability envelope を宣言する。`{}` 内には各メソッドの宣言と、そのメソッド個別の契約 `()` を記述する。

具象実装はコントラクトが定義する capability envelope を超えてはならない。この保証の詳細は §7.6 で定義する。

具象クラスはコントラクトを `: ContractName` で継承し、各メソッドは `: ContractName.MethodName` 構文でコントラクトメソッドを参照する。

```ddc
class SqlRepository : IRepository
{
    public User Load(UserId id) : IRepository.Load
    {
        // implementation
    }
}
```

### 2.6 複数の引数を持つ関数

複数の引数を持つ関数では、各引数のパスを `Read:` に宣言する。

```ddc
public void Transfer(Account from, Account to, Money amount)
(
    Read:
        from.Id
        from.Balance
        to.Id
        amount.Value
    Write:
        from.Balance
        to.Balance
    Call:
        auditLogger.Record()
    Throw:
        InsufficientFundsException
);
```

引数間でパスが衝突する場合（同名フィールドを持つ異なる引数）は、引数名で明確に区別する。

---

## 3. 構造パス記法

### 3.1 パス式の構文

構造パス（structure path）は、オブジェクトグラフ上のアクセス経路を記述する式である。

```ebnf
path            ::= root-identifier path-segment*
root-identifier ::= identifier
path-segment    ::= "." identifier
                  | "[]"
                  | "[]" "." identifier
identifier      ::= ALPHA (ALPHA | DIGIT | "_")*
```

有効なパス例:

```text
order                           → ルート識別子のみ（引数または this フィールドへの参照）
order.Id                        → 単一フィールド
order.Items                     → フィールドとしてのコレクション
order.Items[]                   → Items コレクション全体のトラバーサル
order.Items[].SKU               → Items の全要素に対する SKU の射影
order.Items[].Variants[].Price  → ネストしたトラバーサルの合成
```

### 3.2 ルート識別子

パスのルート識別子は以下のいずれかでなければならない。

- **引数名**: 関数引数として宣言された識別子（例: `order`、`id`）
- **`this`**: 自インスタンスのフィールドまたはプロパティを修飾するキーワード（例: `this.cache`）
- **フィールド名（暗黙的 `this`）**: `this` を省略した自インスタンスのフィールド（例: `cache`）
- **外部依存識別子**: 囲むクラスの依存として宣言されたフィールド（例: `outerRepository`）
- **`::` 型参照プレフィックス**: `::TypeName.field` 形式。`Write:` セクションにのみ使用できる。関数が戻り値として**構築する**値の型に属するフィールドへの依存を宣言する（§6.7 参照）。

上記以外の識別子をルートとするパスは未定義参照（undefined reference）エラーとしなければならない。`::` 型参照パスを `Read:` セクションに記述することはエラーとしなければならない。

### 3.3 `.` アクセサ

`.identifier` は、直前のパス式が示すオブジェクトのフィールドまたはプロパティへのアクセスを意味する。

```text
order.Id       → order が示すオブジェクトのフィールド Id
order.Items    → order が示すオブジェクトのフィールド Items
```

パスが示すオブジェクトが `null` になりうる場合でも、DDC の契約は型安全性の保証ではなく依存境界の宣言であるため、null 可能性は契約の有効性に影響しない。

### 3.4 `[]` の意味論

`[]` はコレクション全体への**全称量化的トラバーサル**を意味する。インデックスによる単一要素アクセスではない。

```text
order.Items[]                   → Items コレクションの全要素を対象とする
order.Items[].SKU               → Items の全要素それぞれの SKU を対象とする
order.Items[].Variants[]        → Items 全要素の Variants コレクション全体を対象とする
order.Items[].Variants[].Price  → Items 全要素の Variants 全要素の Price を対象とする
```

形式的に、`[]` はオプティクス理論における Traversal に対応する。

$$\texttt{order.Items[].SKU} \;\equiv\; \text{Traversal}(\texttt{Order} \to \texttt{Items[]}) \circ \text{Lens}(\texttt{Item} \to \texttt{SKU})$$

この形式化により、パスの合成が代数的に定義可能となり、トラバーサルの合成則が依存パスの合成則となる。

### 3.5 パスの包含関係（Subsumption）

あるパス $p_1$ が別のパス $p_2$ を包含する（subsume する）とは、$p_1$ が表すアクセス範囲が $p_2$ を含むことを意味する。

| 包含するパス | 包含されるパス | 備考 |
| --- | --- | --- |
| `order.Items[]` | `order.Items[].SKU` | コレクション全体への参照はその要素のフィールドを含む |
| `order.Items[]` | `order.Items[].Variants[]` | 同上 |
| `order` | `order.Id` | ルートオブジェクトへの参照は全フィールドを含む |

`Read: order.Items[]` を宣言した関数が `order.Items[].SKU` へのアクセスも許可されるかは実装定義であるが、宣言の意図は明確にするべきである。推奨は、依存をできる限り具体的なパスで宣言することである。過剰に広いパスを宣言することは警告対象となりうる（§15.2 参照）。

### 3.6 パスの有効性検証

適合実装は、宣言されたパスがホスト言語の構造定義（クラス定義・スキーマ等）と一致することを検証するべきである。

**エラーとなるケース:**

| ケース | 例 |
| --- | --- |
| 存在しないフィールドへのパス宣言 | `Read: order.NonExistentField` |
| 非コレクション型への `[]` 適用 | `Read: order.Id[]`（`Id` が非コレクション型の場合） |
| ルート識別子が引数でも `this` でもない | `Read: unknown.Field` |

---

## 4. 契約の意味論

### 4.1 capability envelope

DDC の契約は「依存してよい上限（capability envelope）」を定義するものであり、実行トレースの正確な予測ではない。

```text
未宣言パス   → アクセス不可能        （DDC の核心保証）
宣言済みパス → アクセスしてよい      （必ずアクセスされるとは限らない）
```

これは以下のことを意味する:

- `Read: order.Status` を宣言していても、実装がそれを常に読む必要はない
- 条件分岐により一方のパスのみが実行時に到達可能であっても、両パスを宣言してよい
- 「宣言したが実際には使用しない依存」は警告対象となりうるが（§15.2 参照）、エラーではない

同様に `Throw: IOException` は「必ず `IOException` を投げる」ではなく「投げうる」という能力の上限宣言である。`Read:` / `Write:` も能力の上限であって、実行の記述ではない。

### 4.2 直接作用の原則

DDC は「この関数実装自身が直接行う作用」のみを各セクションに記述する。呼び出し先で発生する作用は記述しない。それらは `Call:` による実効契約の統合（§10 参照）に含まれる。

**直接作用の例:**

```csharp
public void ProcessShipping(Order order)
{
    var id = order.Id;          // 直接 Read: order.Id
    order.Status = "Shipped";   // 直接 Write: order.Status
    logger.Log(id);             // 直接 Call: logger.Log()
    // logger.Log() 内部での Write は ProcessShipping の宣言契約に含まれない
}
```

```ddc
public void ProcessShipping(Order order)
(
    Read:
        order.Id
    Write:
        order.Status
    Call:
        logger.Log()
);
```

### 4.3 アクセス制御の形式化

関数 $f$ と構造パス $p$ について、$f$ による $p$ への**読み取りアクセス**が許可される条件を次のように定義する。

$$\text{readable}(f, p) \iff p \in \text{declared.Read}(f) \lor p \in \text{declared.Write}(f) \lor \exists g \in \text{Call}(f) : p \in \text{effective.Read}(g)$$

$f$ による $p$ への**書き込みアクセス**が許可される条件:

$$\text{writable}(f, p) \iff p \in \text{declared.Write}(f) \lor \exists g \in \text{Call}(f) : p \in \text{effective.Write}(g)$$

すなわち `Write:` に宣言されたパスは読み取りも暗黙的に許可される。これは Read-modify-write パターン（§6.4 参照）を簡潔に扱うためである。

上記どちらも満たさないパス $p$ への $f$ のアクセスはコンパイルエラーまたは静的解析エラーとしなければならない。

### 4.4 条件分岐と契約

条件分岐によって一方のパスのみが実行時に到達可能であっても、静的解析はすべての分岐を対象とする。

```csharp
if (flag)
    order.Status = "A";    // Write: order.Status
else
    order.Notes = "B";     // Write: order.Notes
```

両方を宣言しなければならない。

```ddc
Write:
    order.Status
    order.Notes
```

いずれかのパスのみを宣言することは、一方の分岐に対して未宣言アクセスが発生するため、エラーとなる。

### 4.5 エイリアスと契約

あるパスを別の変数に代入した後、代入先を通じてアクセスする操作も当該パスへの依存として扱う。

```csharp
var items = order.Items;      // items は order.Items へのエイリアス
var sku = items[0].SKU;       // order.Items[].SKU への依存
```

```ddc
Read:
    order.Items[].SKU    // エイリアス経由でも同じパスを宣言する
```

エイリアスの宣言は `Read:` セクションの `as identifier` 構文（§5.6 参照）によるコントラクトレベル宣言のみで行う。body 内の変数代入（`let x = something` 等）はエイリアス宣言として扱わない。宣言されていないローカル変数を介したアクセスは、宣言されたパスへの依存として認識されない。

body 内で別名経由のアクセスを行いたい場合は、コントラクトに `Read: path as name` を宣言する。

---

## 5. Read

### 5.1 定義

`Read:` は、関数実装自身が**直接参照**する構造パスを宣言する。「直接参照」とは、関数本体において当該パスの値を評価することを意味する。

### 5.2 記述対象

**含まれるもの（宣言しなければならないもの）:**

- フィールドへの読み取りアクセス（例: `order.Id`）
- プロパティへの読み取りアクセス
- コレクション全体の参照（例: `order.Items[]`）
- コレクション要素のフィールド参照（例: `order.Items[].SKU`）
- 条件式・比較式でのパス評価（例: `if (order.Status == ...)` における `order.Status`）
- ループ変数のコレクション参照（例: `foreach (var item in order.Items)`）

**含まれないもの（宣言してはならないもの）:**

- 呼び出し先が内部で読む構造パス
- 呼び出し先内部の変数・フィールド
- 定数・リテラル
- ローカル変数（引数でも `this` フィールドでもないもの）

### 5.3 暗黙的 Read

条件式・ループ・比較・メソッドチェーンにおいても、パスを評価することは Read に該当する。

```csharp
if (order.Status == "Active") { ... }    // Read: order.Status
foreach (var item in order.Items) { }   // Read: order.Items[]
var count = order.Items.Count;          // Read: order.Items（または order.Items.Count）
```

これらも `Read:` に宣言しなければならない。

### 5.4 引数を別関数に渡すケース

引数オブジェクトをそのまま別関数に渡す操作は `Call:` で表現し、渡されたオブジェクトの内部への依存は呼び出し先の契約に委ねる。

```csharp
// 引数を内部フィールドに触れずそのまま渡す
void Forward(Order order)
{
    Process(order);
}
```

```ddc
// ok: order の内部フィールドに直接アクセスしていないため Read: order は不要
void Forward(Order order)
(
    Call:
        Process()
);
```

ただし、呼び出し時に引数のフィールドを評価する場合は `Read:` が必要である。

```csharp
void Forward(Order order)
{
    repository.Save(order.Id);    // order.Id を評価している
}
```

```ddc
void Forward(Order order)
(
    Read:
        order.Id
    Call:
        repository.Save()
);
```

### 5.5 エラーとなるケース

適合実装が報告しなければならないエラー:

| ケース | 例 |
| --- | --- |
| 宣言なしのフィールド読み取り | `Read:` に未宣言の `order.Customer` を参照 |
| 呼び出し先内部パスの宣言 | `Read: Database.Tables[]`（`connection.Query()` 内部で読んでいる場合） |
| ホスト言語構造に存在しないパスの宣言 | `Read: order.NonExistentField` |
| 非引数・非 `this` フィールドのルートを持つパス | `Read: unknown.Field` |

### 5.6 パスエイリアス宣言（`as` 修飾子）

`Read:` セクションのパスエントリに `as identifier` を付与して**エイリアス宣言**を行うことができる。

```ddc
Read:
    order.customer.city as city
    order.items[] as items
    order.items[].sku as sku
```

**意味論:**

エイリアス宣言は以下の二つを同時に行う。

1. 当該パスへの `Read:` 依存を宣言する（エイリアスなし宣言と同じ）
2. body スコープで使用できる名前（`identifier`）を当該依存空間にバインドする

宣言されたエイリアスは、body 内でその名前を通じて依存空間にアクセスするための識別子として機能する。エイリアスを通じたアクセスは当該パスへの依存として扱われる（§4.5 参照）。

**制約:**

- `as` 修飾子は `Read:` セクションにのみ記述できる。`Write:` / `Call:` / `Throw:` セクションでの使用はエラーとなる
- `identifier` は当該関数のパラメータ名、および同一 `Read:` ブロック内の他のエイリアス名と衝突してはならない
- 同一パスへのエイリアスなし宣言とエイリアスあり宣言の共存はエラーとなる（重複宣言）
- body 内の変数代入（`let x = something` 等）はエイリアス宣言として扱わない。エイリアスは必ずコントラクトの `as` 宣言で行う

---

## 6. Write

### 6.1 定義

`Write:` は、関数実装自身が**直接変更または構築する**構造パスを宣言する。「直接変更」とは関数本体での値の代入または変更操作を、「直接構築」とは関数が返す新しい値のフィールドを初期化することを意味する。

この定義は「値レベルでは構築と代入は同じ意味を持つ」という原則に基づく。`order.Status = "Shipped"` と `new Order { Status = "Shipped" }` はいずれもフィールドへの値の書き込みである。`Write:` はミューテーションに限らず、こうした**値の変化可能性**を網羅的に追跡する。

### 6.2 Write は Read を暗黙的に含む

`Write:` に宣言されたパスは、読み取りアクセスも暗黙的に許可される（§4.3 参照）。

```ddc
Write:
    order.Status    // order.Status の読み取りと書き込みの両方が許可される
```

したがって、`Write:` と `Read:` の両方に同一パスを記述することは冗長であるが、エラーではない。

### 6.3 記述対象

**含まれるもの（宣言しなければならないもの）:**

- フィールドへの代入（例: `order.Status = ...`）
- プロパティへの代入
- コレクション要素のフィールドへの代入（例: `order.Items[i].SKU = ...`）
- コレクション自体の変更（例: `order.Items.Add(...)`）
- 関数の戻り値として構築する値のフィールド初期化（例: `new Timer { elapsed_ms: 0 }` → `Write: ::Timer.elapsed_ms`）（§6.7 参照）

**含まれないもの（宣言してはならないもの）:**

- 呼び出し先が変更する構造パス
- 外部 API 内部状態（`connection.Execute()` 内部での DB 変更など）
- ローカル変数への代入

### 6.4 Read-modify-write パターン

フィールドを読み取った後に変更する操作（Read-modify-write）では、`Write:` の宣言のみで読み取りも暗黙的に許可される（§6.2 参照）。

```csharp
order.Counter = order.Counter + 1;    // order.Counter の Read かつ Write
```

```ddc
Write:
    order.Counter    // Read も暗黙的に含まれるため、Read: の追加宣言は不要
```

ただし、意図を明示するために `Read: order.Counter` を加えて記述することは許可される。

### 6.5 Write と `[]` の非対称性

```ddc
Write:
    order.Items[].SKU
```

これは「全要素の `SKU` への書き込み権限」を意味し、副作用の影響範囲が契約から直接読み取れる。単一要素のみを変更する実装であっても、`[]` は全称量化的表現であるため宣言の粒度は全要素となる（§3.4 参照）。

単一要素への書き込みに限定した表現は現仕様では提供されない（§15.4 参照）。

### 6.6 エラーとなるケース

| ケース | 例 |
| --- | --- |
| 宣言なしのフィールド変更 | `Write:` に未宣言の `order.Customer` を変更 |
| 宣言なしの構築フィールド初期化 | `Write: ::Timer.elapsed_ms` なしに `Timer { elapsed_ms: 0 }` を構築 |
| 呼び出し先内部パスの宣言 | `Write: Database.Tables[]`（`connection.Execute()` 内部での変更の場合） |
| `readonly` 関数での `Write:` 宣言 | §9 参照（型レベルパスも同様に禁止） |
| `::` 型参照パスを `Read:` に記述 | `Read: ::Timer.elapsed_ms` はエラー |

### 6.7 型レベル Write パス（`::` プレフィックス）

関数が返す新しい値を**構築**する操作（オブジェクト初期化・構造体リテラル）は、既存パラメータへの代入と同様に `Write:` で宣言しなければならない。構築時に初期化するフィールドは `::TypeName.field` 形式（型レベル Write パス）で宣言する。

`::` プレフィックスは、パスが変数（関数引数）ではなく**型（戻り値の型）を起点とする**ことを示す。Rust 等の命名慣習（型は PascalCase）に依存せず、構文的に変数パスと区別できる。

```ddc
// 例 1: 引数なしのコンストラクタ
Timer new_timer()
(
    Write: ::Timer.elapsed_ms
)
{ Timer { elapsed_ms: 0 } }

// 例 2: 既存値から新しい値を構築する
Timer reset_timer(Timer timer)
(
    Write: ::Timer.elapsed_ms
)
{ Timer { elapsed_ms: 0 } }

// 例 3: 複数フィールドを初期化する
Alarm new_alarm(String label, u64 threshold_ms)
(
    Read: label, threshold_ms
    Write: ::Alarm.label, ::Alarm.threshold_ms, ::Alarm.fired
)
{ Alarm { label, threshold_ms, fired: false } }
```

**伝播規則**: 型レベル Write パスは通常の `Write:` パスと同様に LFP により保守的に伝播する。ある関数 $g$ が `Write: ::T.field` を持つ場合、$g$ を `Call:` する関数 $f$ の実効契約にも `::T.field` が含まれる。

```text
// new_timer() を Call: する foo() は、::Timer.elapsed_ms が実効契約に入る
// foo() が readonly ならエラー
```

**`readonly` との関係**: `readonly` 関数の実効 Write 集合に `::` 型レベル Write パスが含まれる場合、通常の `Write:` パスと同様に `ReadonlyViolation` エラーとしなければならない。型レベル構築は「状態を変化させる可能性がある」という理由により、可能性ベース（保守的）で評価される。

---

## 7. Call

### 7.1 定義

`Call:` は、関数実装自身が**直接呼び出す**関数・メソッドを宣言する。「直接呼び出す」とは、関数本体において当該関数のシンボルを呼び出し式として使用することを意味する。

`Call:` は二つの独立した役割を持つ（§7.2、§7.3 参照）。

### 7.2 ホワイトリスト制約

`Call:` に宣言されていない関数の呼び出しはエラーとしなければならない。これにより、暗黙的な依存が構文的に不可能となる。

```ddc
// この宣言を持つ関数は、logger.Log() と repository.Save() のみ呼び出せる
Call:
    logger.Log()
    repository.Save()
```

上記の関数で `clock.Now()` を呼び出す場合、必ず `Call: clock.Now()` を追加しなければならない。追加しない場合はエラーとなる。

### 7.3 統合点（integration point）としての役割

`Call:` に宣言された関数の実効契約は、呼び出し元の実効契約に折り畳まれる（§10 参照）。これにより依存の伝播が契約の代数的合成として閉じる。

```text
f の実効契約 = f の宣言契約 ∪ (f が Call: する全関数の実効契約)
```

この結果、`Call:` 宣言は「どのような副作用が伝播してくるか」の接点（integration point）として機能する。

### 7.4 引数は記述しない

`Call:` には呼び出す関数名のみを記述し、引数は記述しない。呼び出しに使用する引数への依存は `Read:` で表現する。

```ddc
// 正
Call:
    repository.Save()

// 誤
Call:
    repository.Save(user)    // NG: 引数依存は Read: で表現する
```

引数に使用するオブジェクトのフィールドを読み取る場合、そのパスを `Read:` に宣言しなければならない。

```csharp
repository.Save(user.Id, user.Name);
```

```ddc
Read:
    user.Id
    user.Name
Call:
    repository.Save()
```

### 7.5 外部 API

TCP・HTTP・FileSystem・OS syscall・データベースアクセス等の外部 API 呼び出しもすべて `Call:` で宣言しなければならない。DDC はこれらを特別扱いしない。

```ddc
Call:
    Kernel.Network.Send()
    FileSystem.WriteAllBytes()
    Database.Execute()
    DateTime.Now()
    Random.Next()
```

外部 API を `Call:` に宣言しないことは、当該依存を隠蔽することになり、DDC の核心保証（§14.1 参照）に反する。

### 7.6 コントラクト越しの呼び出し

```ddc
Call:
    IRepository.Load()
```

コントラクトを通じた呼び出しでは、具象実装の契約ではなくコントラクトの宣言契約を実効契約に統合する。これは以下の保証をもたらす:

- 呼び出し元は具象実装の詳細に依存しない
- 具象実装を差し替えても、呼び出し元の実効契約は変化しない
- 具象実装がコントラクトの capability envelope を超えた依存を持てないことが、コントラクトの意味論的保証となる

具象実装が対応するコントラクトの capability envelope を超えた依存を宣言している場合、適合実装はエラーを報告しなければならない。

### 7.7 コンストラクタ呼び出し

オブジェクトの構築（`new` 式）も `Call:` として宣言しなければならない（§11.2 参照）。

```ddc
Call:
    Query.Constructor()
```

宣言なしの `new` 式はエラーとしなければならない。コンストラクタへの引数に使用するパスは `Read:` に宣言しなければならない。

```csharp
var query = new Query(this.connectionString);
```

```ddc
Read:
    this.connectionString
Call:
    Query.Constructor()
```

### 7.8 同一クラス内のメソッド呼び出し

同一クラス内のメソッドを呼び出す場合も `Call:` に記述しなければならない。`this` メソッドの呼び出しを暗黙的に許可することはない。

```ddc
Call:
    this.Validate()
    this.BuildQuery()
```

### 7.9 静的メソッド

静的メソッドの呼び出しも `Call:` に記述しなければならない。

```ddc
Call:
    DateTime.Now()
    Guid.NewGuid()
    Environment.GetEnvironmentVariable()
```

### 7.10 エラーとなるケース

| ケース | 例 |
| --- | --- |
| 宣言なしの関数呼び出し | `Call:` 未宣言の `clock.Now()` を呼び出す |
| 宣言なしの `new` 式 | `Call: Query.Constructor()` なしに `new Query(...)` を使用 |
| 具象実装がコントラクトを超過 | 実装が `IRepository.Load()` の capability envelope を超える依存を宣言 |
| 引数を `Call:` に記述 | `Call: repository.Save(user)` |

---

## 8. Throw

### 8.1 定義

`Throw:` は、関数実装自身が**例外生成点（throw site）**となる例外型を宣言する。「例外生成点」とは、関数本体に `throw new ExceptionType(...)` が存在するフレームを意味する。

### 8.2 意味論

`Throw:` は「この関数フレームが例外生成点である」ことを意味する。これは「必ず例外を投げる」という記述ではなく、「投げうる」という能力の宣言である（§4.1 参照）。

```csharp
if (id == null)
    throw new ValidationException("id is null");
```

```ddc
Throw:
    ValidationException
```

`Throw:` に宣言する例外型は完全修飾名またはホスト言語のスコープで解決可能な名前でなければならない。基底クラスを宣言することで派生例外を包括的に宣言してよい。宣言された基底クラスの派生型を throw することは宣言を満たす。

### 8.3 呼び出し先例外は記述しない

呼び出し先が投げる例外は `Throw:` に記述してはならない。それらは `Call:` による実効契約の統合（§10 参照）に含まれる。

```ddc
// NG: connection.Query() が投げる例外を Throw: に記述している
Throw:
    SqlException    // connection.Query() の内部でのみ throw される場合

// ok: SqlException の伝播は Call: connection.Query() の実効契約に含まれる
Call:
    connection.Query()
```

### 8.4 例外の再スロー

`catch` ブロックで例外を再スロー（`throw`）する場合、再スローされる例外型を `Throw:` に宣言しなければならない。

**同型の再スロー:**

```csharp
catch (IOException ex)
{
    LogError(ex);
    throw;    // IOException を再スロー
}
```

```ddc
Throw:
    IOException
Call:
    LogError()
```

**変換スロー（ラップして別型として throw）:**

```csharp
catch (SqlException ex)
{
    throw new RepositoryException(ex);    // 変換スロー
}
```

```ddc
Throw:
    RepositoryException    // 変換後の型を宣言する
// SqlException は Call: chain に含まれるため Throw: への記述は不要
```

**catch して握り潰す場合（rethrow なし）:**

```csharp
catch (IOException ex)
{
    LogError(ex);
    // throw しない
}
```

この場合、`IOException` を `Throw:` に宣言する必要はない。ただし `LogError()` の呼び出しは `Call:` に記述しなければならない。

### 8.5 エラーとなるケース

| ケース | 例 |
| --- | --- |
| 宣言なしの例外生成 | `Throw:` 未宣言の `new ValidationException()` を throw |
| 呼び出し先例外の `Throw:` 宣言 | `connection.Query()` のみが throw する `SqlException` を `Throw:` に記述 |

---

## 9. readonly 関数

### 9.1 定義

`readonly` は関数修飾子であり、その関数が**副作用（状態変更）を直接持たず、副作用を持つ関数を呼び出さない**ことを宣言する。

DDC における `readonly` は単なる immutable 制約ではなく、「副作用権限」を構文レベルで除去する。

```ddc
public readonly User Find(UserId id)
(
    Read:
        id.Value
    Call:
        repository.Load()
);
```

### 9.2 readonly の制約

`readonly` 関数では以下が禁止される。違反は適合実装がエラーとしなければならない。

| 禁止事項 | 理由 |
| --- | --- |
| `Write:` セクションの宣言（変数パス） | 既存値の変更を直接行うことは副作用であり、readonly でない |
| `Write:` セクションの宣言（`::` 型レベルパス） | 値の構築も値変化可能性の一形態であり、readonly 制約に反する（§6.7 参照） |
| `Write:` 宣言を持つ非 `readonly` 関数の `Call:` | 呼び出し先での副作用は readonly 制約の迂回となる |

```ddc
// NG: Write: 宣言を持つ関数を Call: している
public readonly User Find(UserId id)
(
    Read:
        id.Value
    Call:
        cache.Update()    // NG: cache.Update() が Write: を宣言している場合
);
```

### 9.3 readonly の推移的閉包

`readonly` 関数が `Call:` できるのは、次のいずれかに該当する関数のみである。

```text
f が readonly ならば、f の Call: に含まれるすべての g は
  (1) readonly として宣言されている、または
  (2) [kernel] アノテーションが付与されている
```

`[kernel]` 関数は `effective.Write` を呼び出し元へ伝播しない（§12.6 参照）ため、`readonly` 制約の迂回とならない。

コントラクト越しの呼び出し（§7.6 参照）では、コントラクトメソッドが `readonly` として宣言されている場合、呼び出し元の `readonly` 関数からそのコントラクトメソッドを呼び出すことができる。ただし具象実装が `readonly` 制約を満たしていることが保証されなければならない。

### 9.4 readonly と外部 I/O

`readonly` 関数は `Read:` と `Call:` の使用は制限されない。例えばデータベース読み取り（クエリ）を `Call: repository.Load()` として宣言できる。

```ddc
public readonly User Find(UserId id)
(
    Read:
        id.Value
    Call:
        repository.Load()    // DB への読み取りクエリは許可される
);
```

**注意**: `repository.Load()` が DB への I/O を伴う場合、それは副作用の一種であるが、DDC の `readonly` はホスト言語上の**状態変更**（Write）のみを制限する。純粋性（Pure）の保証はより高次の制約として別途定義される余地がある（要検討）。

### 9.5 エラーとなるケース

| ケース | 例 |
| --- | --- |
| `readonly` 関数での `Write:` 宣言（変数パス） | `readonly` 修飾の関数が `Write: this.cache` を宣言 |
| `readonly` 関数での `Write:` 宣言（型レベルパス） | `readonly` 修飾の関数が `Write: ::Timer.elapsed_ms` を宣言 |
| `readonly` 関数が非 `readonly` 関数を `Call:` | `readonly` 関数が `Write:` を持つ `Process()` を呼び出す（型レベル Write を持つ場合も同様） |

---

## 10. 依存の合成と実効契約

### 10.1 実効契約の定義

ある関数 $f$ の**実効契約（effective contract）**は、宣言された契約と呼び出し先の実効契約の和集合として再帰的に定義される。

$K$ を `[kernel]` アノテーションが付与された関数の全体集合とする（§12.6 参照）。`[kernel]` 関数は検証スコープ境界を形成し、`effective.Read` / `effective.Write` を呼び出し元へ伝播しない。

各セクション別の実効契約は次のように定義される。

$$\text{effective.Read}(f) = \text{declared.Read}(f) \cup \bigcup_{\substack{g \in \text{Call}(f) \\ g \notin K}} \text{effective.Read}(g)$$

$$\text{effective.Write}(f) = \text{declared.Write}(f) \cup \bigcup_{\substack{g \in \text{Call}(f) \\ g \notin K}} \text{effective.Write}(g)$$

$$\text{effective.Throw}(f) = \text{declared.Throw}(f) \cup \bigcup_{g \in \text{Call}(f)} \text{effective.Throw}(g)$$

$$\text{effective.Call}(f) = \text{declared.Call}(f) \cup \bigcup_{g \in \text{Call}(f)} \text{effective.Call}(g)$$

$g \in K$ に対しては `Call:` セクションが存在しないため $\text{effective.Throw}(g) = \text{declared.Throw}(g)$ が成立する。ここで $\text{Call}(f)$ は $f$ の `Call:` セクションに宣言された関数の集合を示す。

### 10.2 例: 実効契約の計算

```ddc
// GetSomeData の宣言契約
public Data GetSomeData()
(
    Read:
        repository.ConnectionString
    Write:
        cache.Entry
);

// ProcessShipping の宣言契約
public void ProcessShipping(Order order)
(
    Read:
        order.Id
        order.Items[].SKU
    Write:
        order.Status
    Call:
        outerRepository.GetSomeData()
    Throw:
        IOException
);
```

`ProcessShipping` の実効契約:

| セクション | 内容 |
| --- | --- |
| Read | `order.Id`、`order.Items[].SKU`、`repository.ConnectionString` |
| Write | `order.Status`、`cache.Entry` |
| Call | `outerRepository.GetSomeData()` |
| Throw | `IOException` |

### 10.3 Call Reachability

capability は Call Reachability によって定義される。

```text
A が B を Call し、
B が C を Call するなら、
A は C の capability を持つ。
```

変更影響範囲の解析は `Call:` チェーンを辿ることで静的に完結する。あるパス $p$ を `Write:` に宣言している関数 $h$ が存在する場合、$h$ を直接または間接的に `Call:` するすべての関数が $p$ への Write capability を実効契約として保持する。

### 10.4 再帰・相互再帰の扱い

再帰関数の実効契約は最小不動点として定義される。

$$\text{effective}(f) = \text{lfp}\left(\lambda X.\; \text{declared}(f) \cup \bigcup_{g \in \text{Call}(f)} X(g)\right)$$

再帰 $f \to f$（自己再帰）では、$\text{effective}(f)$ はイテレーションが収束するまで展開される。構造パスの集合は有限閉包を持つため（型定義が有限である限り）、停止性は保証される。

相互再帰（$f \to g \to f$）の場合も同様に最小不動点として計算される。

```ddc
public void Traverse(Node node)
(
    Read:
        node.Value
        node.Children[]
    Call:
        this.Traverse()    // 自己再帰
);
```

この場合の実効契約は宣言契約と等しい（自己参照が新たなパスを追加しないため）。

### 10.5 コントラクト経由の合成

呼び出し先がコントラクト経由の場合、実効契約にはコントラクトの宣言契約が統合される（具象実装の宣言契約ではない）。

```ddc
// 呼び出し元
Call:
    IRepository.Load()

// IRepository コントラクトの宣言契約が実効契約に統合される
// 具象実装 SqlRepository.Load() の宣言契約は統合されない
```

---

## 11. Dependency Closure

### 11.1 原則

DDC の依存グラフは閉じていなければならない。関数の実装中に発生するすべての依存は、直接または `Call:` を通じた間接的な宣言として契約に含まれなければならない。

$$\forall \text{アクセス} \, a \text{（f が実装中に行う）}: a \text{ に対応するパスまたは呼び出しが } \text{declared}(f) \text{ または } \text{effective}(\text{Call}(f)) \text{ に含まれる}$$

これが満たされない場合、未宣言依存（undeclared dependency）エラーとしなければならない。

### 11.2 Hidden Construction の禁止

未宣言のオブジェクト構築は禁止される。オブジェクトを構築する場合、対応するコンストラクタ呼び出しを `Call:` に宣言しなければならない。

```csharp
// NG: Call: Query.Constructor() が存在しない場合
var query = new Query(connectionString);
```

```ddc
// 正
Read:
    this.connectionString
Call:
    Query.Constructor()
```

コンストラクタへの引数に使用するパスは `Read:` に宣言しなければならない。

### 11.3 ファクトリメソッドの扱い

`new` の代わりにファクトリメソッドを使用する場合、ファクトリメソッドを `Call:` に宣言することで Dependency Closure を満たす。コンストラクタの直接呼び出しと等価の扱いを受ける。

```ddc
Call:
    QueryFactory.Create()
```

ファクトリメソッド内部での構築は、そのメソッドの契約に含まれる。

### 11.4 クロージャ・ラムダ

ラムダ式・匿名関数が外部スコープの変数・フィールドをキャプチャしてアクセスする場合、そのアクセスは囲む関数の直接作用として扱い、対応するパスを宣言しなければならない。

```csharp
var threshold = this.config.MaxRetries;        // Read: this.config.MaxRetries
var filtered = items.Where(x => x.Count > threshold);    // Read: items[].Count
```

```ddc
Read:
    this.config.MaxRetries
    items[].Count
```

ラムダ内部での関数呼び出しも同様に、囲む関数の `Call:` に含めなければならない。

```csharp
items.ForEach(item => logger.Log(item.Id));
```

```ddc
Read:
    items[].Id
Call:
    logger.Log()
```

### 11.5 静的初期化子

静的初期化子（クラス初期化子）の呼び出しも `Call:` として宣言しなければならない。静的初期化子が副作用を持つ場合、それは暗黙的依存となるため、DDC は静的初期化子への依存を宣言可能な構文として提供する（具体的な構文は実装定義）。

---

## 12. レイヤー構造

### 12.1 層の定義

DDC では、副作用の宣言方式に基づき、以下の三層を識別する。これは DDC が強制する制約ではなく、DDC 契約によって自然に成立するアーキテクチャパターンである。

| 層 | 副作用の宣言方式 | 典型的な責務 |
| --- | --- | --- |
| Kernel Layer | `[kernel]` アノテーション付き。DDC 検証スコープ外 | OS・ネットワーク・ストレージへの直接操作 |
| Middleware Layer | `Call:` による `[kernel]` 関数への委譲 | プロトコル変換・バッファリング・キャッシュ |
| Application Layer | `Call:` による orchestration のみ | ユースケース・ビジネスロジック |

低レイヤーほど concrete effect を直接宣言し、高レイヤーほど `Call:` による委譲のみになる。

### 12.2 Kernel Layer

Kernel Layer の関数は `[kernel]` アノテーションによって宣言される。これらの関数は DDC 検証スコープ外であり、内部の Read/Write は検証されない（§12.6 参照）。

```ddc
[kernel]
public void Send(byte[] bytes)
(
    Throw:
        NetworkDisconnectedException
);
```

特徴:

- `[kernel]` アノテーションが付与されており、DDC 検証スコープ境界を形成する
- `Read:` / `Write:` セクションを持たない（宣言してはならない）
- `Throw:` に OS・ハードウェア起因の例外が含まれる
- 呼び出しは通常の `Call:` で表現する
- 言語実装・コアライブラリのみが `[kernel]` を宣言できる（§12.6 参照）

### 12.3 Middleware Layer

Middleware Layer は副作用を直接持たず、Kernel への `Call:` に委譲する。

```ddc
public void Send(Packet packet)
(
    Read:
        packet.Bytes[]
    Call:
        network.Send()
);
```

特徴:

- `Write:` を直接宣言しない（副作用は `Call:` 経由）
- Kernel または下位 Middleware を `Call:`
- プロトコル変換・フィルタリング・バッファリングロジックを担う

### 12.4 Application Layer

Application Layer は orchestration のみを担う。

```ddc
public readonly User Find(UserId id)
(
    Read:
        id.Value
    Call:
        repository.Load()
);
```

特徴:

- `Write:` を直接宣言しない
- Middleware または Repository コントラクトを `Call:`
- ビジネスロジック・ユースケースを調整する
- 多くの場合 `readonly` として宣言される

### 12.5 層間制約と違反の検出

DDC 契約を検査することで、層間の違反を静的に検出できる。

**検出可能な違反例:**

| 違反パターン | 検出方法 |
| --- | --- |
| Application Layer が `[kernel]` 関数を直接 `Call:` | Application 関数の `effective.Call` に `[kernel]` 関数が含まれ、かつ Middleware を経由していない |
| Middleware が Application を逆依存 | `Call:` チェーンに循環が生じる |
| Kernel Layer が上位層を呼び出す | `[kernel]` 関数の `Call:` セクションに Application/Middleware Layer の関数が参照されている |

これらの検出は DDC の静的解析ツールが実効契約の内容を検査することで実現する。強制するかは実装ポリシーによる。

### 12.6 `[kernel]` アノテーション

#### 定義

`[kernel]` アノテーションは、関数を**DDC 検証スコープ境界（verification scope boundary）**として指定する。`[kernel]` が付与された関数の内部実装は DDC の静的検証対象外となる。これは、ホスト OS・ハードウェアドライバ・ランタイム組み込み関数のように、DDC のパス検証が不可能または無意味な実装レイヤーを扱うために導入される。

#### 内部契約規則

`[kernel]` 関数の契約ブロックは次の規則に従わなければならない。

| 規則 | 詳細 |
| --- | --- |
| `Read:` 宣言禁止 | `[kernel]` 関数に `Read:` セクションを記述してはならない |
| `Write:` 宣言禁止 | `[kernel]` 関数に `Write:` セクションを記述してはならない |
| `Call:` 宣言禁止 | `[kernel]` 関数に `Call:` セクションを記述してはならない |
| `Throw:` 宣言可 | OS・ハードウェア起因の例外型を宣言できる |

```ddc
// ok: [kernel] 関数の契約は Throw: のみ
[kernel]
public void WriteBlock(byte[] data, long offset)
(
    Throw:
        IOException
        DeviceNotFoundException
);

// NG: [kernel] 関数に Read: / Write: は宣言できない
[kernel]
public void WriteBlock(byte[] data, long offset)
(
    Read:
        data[]          // NG
    Write:
        Disk.Block      // NG
);
```

#### 伝播規則

`[kernel]` 関数を `Call:` に宣言した場合、呼び出し元の実効契約への寄与は次のように制限される。

| セクション | 伝播 |
| --- | --- |
| `effective.Read` | **伝播しない**（呼び出し元の `effective.Read` に加算されない） |
| `effective.Write` | **伝播しない**（呼び出し元の `effective.Write` に加算されない） |
| `effective.Throw` | **伝播する**（`declared.Throw` が呼び出し元へ折りたたまれる） |

この設計により、Middleware Layer は `[kernel]` 関数を `Call:` しても自身の `effective.Write` が汚染されず、`readonly` 制約との矛盾が生じない。

#### 宣言制限ポリシー

`[kernel]` アノテーションは**言語実装・コアライブラリのみが宣言できる**。適合実装は、これら以外のコードが `[kernel]` を宣言することをエラーとしなければならない。

何をもって「言語実装・コアライブラリ」とするかは適合実装が定義する（例: 特定のアセンブリ属性、ビルド時のフラグ等）。

#### 使用警告ポリシー

`[kernel]` 関数を `Call:` に含む関数に対し、適合実装は警告を発するべきである（§15.8 参照）。警告の意図は、Application Layer が `[kernel]` 関数を直接 `Call:` することを抑止し、Middleware による適切なラッピングを促すことである。

#### エラーとなるケース

| ケース | 例 |
| --- | --- |
| 非コアライブラリが `[kernel]` を宣言 | アプリケーションコードに `[kernel]` を付与 |
| `[kernel]` 関数に `Read:` を宣言 | `[kernel]` 関数の契約に `Read:` セクションが存在する |
| `[kernel]` 関数に `Write:` を宣言 | `[kernel]` 関数の契約に `Write:` セクションが存在する |
| `[kernel]` 関数に `Call:` を宣言 | `[kernel]` 関数の契約に `Call:` セクションが存在する |

---

## 13. Effect Propagation

### 13.1 伝播規則

各セクションの効果の伝播規則を以下に示す。

| セクション | 直接作用 | `Call:` による伝播（非 `[kernel]`） | `Call:` による伝播（`[kernel]`） |
| --- | --- | --- | --- |
| `Read` | 宣言した関数自身の直接 Read | `effective.Read` が折り畳まれる | **伝播しない** |
| `Write` | 宣言した関数自身の直接 Write | `effective.Write` が折り畳まれる | **伝播しない** |
| `Call` | 宣言した関数自身の直接 Call | `effective.Call` が折り畳まれる（推移的閉包） | `effective.Call` が折り畳まれる（推移的閉包） |
| `Throw` | 宣言した関数自身の throw site | `effective.Throw` が折り畳まれる | `declared.Throw` が折り畳まれる |

### 13.2 伝播の形式的定義

$K$ を `[kernel]` 関数の全体集合とし（§10.1、§12.6 参照）、セクション $s \in \{\text{Read}, \text{Write}, \text{Call}, \text{Throw}\}$ について定義する。

$s \in \{\text{Read}, \text{Write}\}$ の場合（`[kernel]` から伝播しない）:

$$\text{effective}_s(f) = \text{declared}_s(f) \cup \bigcup_{\substack{g \in \text{Call}(f) \\ g \notin K}} \text{effective}_s(g)$$

$s = \text{Throw}$ の場合（`[kernel]` の `declared.Throw` のみ伝播する）:

$$\text{effective}_{\text{Throw}}(f) = \text{declared}_{\text{Throw}}(f) \cup \bigcup_{g \in \text{Call}(f)} \text{effective}_{\text{Throw}}(g)$$

$s = \text{Call}$ の場合（すべての呼び出しが推移的に展開される）:

$$\text{effective}_{\text{Call}}(f) = \text{declared}_{\text{Call}}(f) \cup \bigcup_{g \in \text{Call}(f)} \text{effective}_{\text{Call}}(g)$$

### 13.3 伝播の例

```text
A → B → C
A.Call: B()
B.Call: C()
C.Write: database.Records[]

→ effective.Write(C) = {database.Records[]}
→ effective.Write(B) = {database.Records[]}
→ effective.Write(A) = {database.Records[]}
```

A は `database.Records[]` への Write capability を保持する（C を通じた推移的伝播）。

### 13.4 Throw の伝播

```text
A → B → C
C.Throw: SQLException

→ effective.Throw(B) ∋ SQLException
→ effective.Throw(A) ∋ SQLException
```

`A` は `SQLException` の伝播経路上に存在する。`A` が `SQLException` を処理するかどうかは実装の問題であり、DDC は処理の強制はしない。ただし `A` の実効契約には `SQLException` が含まれるため、`A` の呼び出し元は `SQLException` の伝播を把握できる。

---

## 14. 設計原則

### 14.1 核心保証: hidden capability の禁止

DDC の核心的な設計目標は、**隠れた capability（hidden capability）の禁止**である。以下のリソースへのアクセスはすべて、対応する `Call:` への明示的宣言なしに関数内に存在できない。

| 隠れた capability の例 | DDC による要件 |
| --- | --- |
| Hidden I/O | `Call: FileSystem.Write()` 等の宣言が必須 |
| Hidden global state | `Read:` / `Write:` での宣言が必須 |
| Hidden singleton | `Call: Singleton.GetInstance()` の宣言が必須 |
| Hidden network access | `Call: Kernel.Network.Send()` 等の宣言が必須 |
| Hidden clock access | `Call: DateTime.Now()` の宣言が必須 |
| Hidden randomness | `Call: Random.Next()` の宣言が必須 |

これらが `Call:` なしに存在できないことで:

- テストにおける副作用の差し替えが宣言から導出可能になる
- コードレビューで「この関数は本当に時刻を使うのか」を宣言から即座に確認できる
- 静的解析で全依存経路を網羅できる

**核心保証の前提条件**: この保証は、適合実装が関数の body を構文解析し、body 内の実際のフィールドアクセス・関数呼び出し・例外生成が宣言契約と一致することを検証する場合にのみ成立する。宣言契約の形式的整合性のみを検証し、body の実装内容を検証しない実装は、宣言と実装が乖離した状態を静かに許容する。適合実装は body レベルの静的検証を行わなければならない。

### 14.2 変更影響範囲の静的解析可能性

DDC 契約が揃った環境では、あるパス $p$ を変更した場合の影響範囲を次の手順で静的に特定できる。

1. $p$ を `Write:` に宣言している関数の集合 $W$ を求める
2. $W$ の関数を直接または間接的に `Call:` している関数を Call Reachability チェーンで展開する
3. 展開された関数集合が変更の影響候補となる

この解析は `Call:` チェーンの有向グラフ上の到達可能性問題であり、静的に完結する。

### 14.3 DTO・ViewModel 増殖の排除

DDC は型を分割しない。`Order order` というドメイン型を保ちながら、依存する構造パスのみを宣言する。

```ddc
// DTO を定義せず、Order のまま使いながら依存を制限できる
public void ProcessShipping(Order order)
(
    Read:
        order.Id
        order.Items[].SKU
);
```

これにより「依存境界のためだけに新しい型を定義する」という DTO・ViewModel・Adapter の増殖を防ぐ。

### 14.4 到達可能性と依存可能性の分離

DDC の最重要原則:

```text
構造が public に存在する（到達可能）≠ その構造へ依存してよい（依存可能）
```

従来の OOP では「到達可能 = 依存可能」であったが、DDC はこれを解除する。型への参照を保持していても、契約に宣言していない構造パスへの依存は禁止される。

この原則により、以下が達成される:

- 参照の保持と依存の成立が分離される
- 型定義を変更せず、依存境界を宣言によって制御できる
- 既存の型システムへの変更を最小化しながら依存制御を導入できる

### 14.5 契約による自己文書化

DDC の契約は設計意図の記述でもある。

```ddc
public void ProcessShipping(Order order)
(
    Read:
        order.Id
        order.Items[].SKU
    Write:
        order.Status
);
```

この宣言は「ProcessShipping は注文の ID と SKU を読み、ステータスを更新する」という設計意図を機械的に検証可能な形で記述する。コメントや設計書に記述していた依存情報が、契約として一次情報となる。

---

## 15. 実装上の考慮事項

以下は実用化に向けた技術的課題および設計課題であり、本仕様の規範要件ではない。

### 15.1 自動契約生成

既存コードから初期契約を推論するツール。DDC の新規導入時に、既存コードへの手動契約付与コストを削減するために必要となる。

**推論に必要な解析:**

- **到達可能性解析**: 関数から直接参照されるフィールド・メソッドの列挙
- **エイリアス解析**: ローカル変数を通じた間接参照の追跡
- **例外フロー解析**: `throw` サイトの特定と例外型の推定

**トレードオフ:**

- 保守的推論（偽陽性多め）vs 精密推論（計算コスト高）
- エイリアス解析の精度はコードの複雑さに依存する
- 自動生成された契約は人間によるレビューの対象とするべきであり、そのまま確定させるべきではない

DDC の宣言コストはすでに大規模コードベースで発生している（人間の頭の中で非形式的に支払われている）。自動契約生成はこのコストをツール側へ移転する手段である。

### 15.2 未使用依存の検出（Unused Dependency Pruning）

宣言されたが実際には使用されないパスの検出と警告。

**対象ケース:**

- `Read: order.Customer` を宣言しているが、実装内で参照していない
- `Call: foo()` を宣言しているが、`foo()` の実効契約が呼び出し元の宣言契約に何も追加しない

**注意:** 過剰宣言（over-declaration）は DDC 違反ではなく警告対象である（§4.1 参照）。条件分岐により実行時には使用されないが静的解析上は宣言が必要なパスも存在するため、警告の抑制メカニズムを提供するべきである。

### 15.3 IDE 補完と可視化

**契約入力補完:**

- ホスト言語の型定義から有効な構造パスを補完候補として提示する
- `Read: order.` と入力した際に `Order` クラスのフィールド一覧を表示する

**実効契約の可視化:**

- 関数の実効契約（宣言契約 + `Call:` チェーンで展開された全依存）をホバー表示する
- Call Reachability チェーンを依存グラフとして可視化する

**変更影響解析:**

- あるフィールドを変更した際に影響を受ける関数の一覧をリアルタイムに表示する
- `effective.Write` に特定パスを含む関数をすべて列挙する

### 15.4 `[]` の粒度の粗さ

現仕様の `[]` は全称量化的トラバーサルのみを表現するため、「インデックス $i$ の要素のみ読む」という依存を表現できない。

```text
現在:    order.Items[]     → 全要素（全称量化）
未定義:  order.Items[i]    → 単一要素（存在量化）
```

実用コードでは単一要素アクセスのパターンも頻出であり、常に `[]` で宣言することは過剰宣言の常態化を招く。単一要素アクセス構文の追加を検討する余地がある。

**候補構文:**

```text
order.Items[*]    → 全称量化（現在の [] と同義、明示的表記）
order.Items[_]    → 存在量化（何らかのインデックスによる一要素アクセス）
```

### 15.5 ジェネリクス・高階関数との相互作用

型パラメータ `T` が具体化されない時点での構造パス契約の宣言方法が未定義である。

**問題点:**

```ddc
// T が具体化されていない時点では T の内部フィールドを宣言できない
public List<TResult> Map<T, TResult>(List<T> list, Func<T, TResult> selector)
(
    Read:
        list[]          // T の内部は不明
    Call:
        selector()      // selector の契約は具体化時に決まる
);
```

**候補アプローチ:**

- 型パラメータに対する契約パラメータ（contract parameter）の導入
- DDC 契約を型クラス制約として表現する
- 高階関数の `Call:` に渡される関数の契約を呼び出し元が指定する仕組み

これは未解決の設計課題であり、本仕様では規定しない。

### 15.6 具象実装変更時の契約伝播

具象実装を直接呼び出す場合、実装が変わりアクセスするフィールドが増えると実効契約が変化し、呼び出し元に連鎖的な再宣言が求められる可能性がある。

**推奨方針:**

- 上位層はコントラクトを通じて呼び出し、具象実装の変更を実効契約の変化から分離する（§7.6 参照）
- 具象直接呼び出しを行う場合、契約変化の影響範囲を IDE ツールで可視化することで変更コストを許容可能なレベルに抑える

### 15.7 インクリメンタル再解析

大規模コードベースでは、契約変更時にすべての実効契約を再計算することはコストが高い。

**効率化の方針:**

- **差分計算**: 変更された宣言契約から実効契約への影響を差分として追跡する
- **トポロジカル順序に基づく更新**: `Call:` グラフのトポロジカル順序で更新を下流に伝播する
- **キャッシュ無効化**: 実効契約のキャッシュを宣言契約の変更で無効化し、必要時のみ再計算する

### 15.8 `[kernel]` 使用警告の抑制

`[kernel]` 関数を `Call:` する側には警告が発される（§12.6 参照）。この警告は Middleware Layer で意図的に `[kernel]` を呼び出す場合には抑制が必要となる。

**抑制方針:**

- 抑制の具体的な構文は実装定義とする（例: `[suppress(kernel-call)]` アノテーション等）
- 抑制は関数単位またはモジュール単位で指定できるべきである
- Middleware として意図的に `[kernel]` を呼び出す場合、抑制とともにその意図をコメントまたは設計文書に記述することを推奨するべきである
- 抑制されていない `[kernel]` 直接呼び出しがレポートに残ることで、意図しない依存の侵入を検出できる
