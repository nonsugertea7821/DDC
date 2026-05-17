# DDC — Declarative Dependency Control

**DDC** は、関数が依存する構造パスを静的契約として宣言し、未宣言依存へのアクセスを禁止する言語・ツールチェーンです。

## 概要

DDC では、各関数の `Read` / `Write` / `Call` / `Throw` セクションに依存パスを明示的に宣言します。宣言されていないパスへのアクセスはコンパイル時に禁止されるため、依存関係が常にコードに明文化された状態を保てます。

```ddc
void process_order(AppCtx ctx)
(
    Read:
        ctx.order.id,
        ctx.user.name
    Write:
        ctx.order.status
    Call:
        notify_user()
) {
    // Rust 実装本体
}
```

## クレート構成

| クレート | 役割 |
|----------|------|
| `ddc-syntax` | `.ddc` ファイルの字句解析・構文解析（Lexer / Parser） |
| `ddc-core` | 依存グラフ（`DependencyGraph`）・契約合成・LFP 検証 |
| `ddc-build` | ビルドスクリプト統合・`syn` による Rust 本体解析 |
| `ddc-codegen` | `proc-macro` 向け `TokenStream` 生成 |

## ステータス

| フェーズ | 内容 | 状態 |
|----------|------|------|
| 1 | ddc-syntax パーサー | ✅ 完了 |
| 2 | DependencyGraph 構築 | ✅ 完了 |
| 3 | LFP + readonly/kernel 検証 | ✅ 完了 |
| 4 | BodyAnalyzer（syn body 検証） | ✅ 完了 |
| 5 | ddc-codegen | ✅ 完了 |
| 6 | 型レベル Write パス（`::` 構文） | ✅ 完了 |

## 現在の作業

全実装フェーズ完了。現在は仕様書・設計ドキュメント（`draft/`）の清書中。

## ビルド

```sh
cd draft/crate
cargo build
cargo test
```

## ライセンス

[MIT](../LICENSE)
