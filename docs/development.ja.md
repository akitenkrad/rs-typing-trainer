[English](development.md) | **日本語**

# 開発者向け情報

## ビルド

```sh
cargo build --release
cargo run --release
cargo run --release -- --lang ja
```

`build.rs` は Git の短いコミットハッシュ，UTC のビルド日，コンパイル対象，Cargo のプロファイルを埋め込みます．`SOURCE_DATE_EPOCH` が設定されている場合は，その値をビルド時刻に使います．`rs-typing-trainer --version`（または `-V`）を実行すると，パッケージのバージョンと，埋め込んだコミット・ビルド日を確認できます．

rs-typing-trainer は Rust edition 2024 で，描画に [ratatui](https://ratatui.rs) 0.30，コマンドラインの解析に [clap](https://docs.rs/clap) 4 を使っています．端末へのアクセスには `ratatui::crossterm` の再エクスポートを使い，ratatui とアプリケーションが互換性のない crossterm を選ぶのを防ぎます．

## ソース構成

| ファイル | 役割 |
|---|---|
| `src/main.rs` | コマンドの振り分け，端末の初期化，イベントループ |
| `src/lib.rs` | バイナリと結合テストへモジュールを公開 |
| `src/cli.rs` | play・replay・stats・export の clap 定義 |
| `src/i18n.rs` | 利用者に表示する英語と日本語の文言 |
| `src/config.rs` | v2 TOML の読み込み，v1 の読み替え，検証，コメントを保つ書き戻し |
| `src/data/` | 同梱 TSV の組み込みと利用者データの読み込み |
| `data/` | 生成した英日出題データ，manifest，権利表示 |
| `tools/gen-data/` | 独立した決定的生成器と点検ファイル |
| `src/layout/` | 物理配置，同梱 TOML，逆引き，入力の読み替え |
| `src/app.rs` | 画面の状態と UI・エンジン間の接続 |
| `src/engine/` | 出題項目，完全一致・ローマ字判定器，モード生成器，決定的に再生できるセッション進行 |
| `src/game/` | 入力の分類，タイマー，単語の生成 |
| `src/analysis.rs` | 項目ログと打鍵ログからの指標計算 |
| `src/analysis/` | キー・遷移・指の詳細指標と CSV 分析 |
| `src/bench.rs` | 決定的な `bench-v1` 平行版の生成 |
| `src/metrics/` | 結果画面と v1 互換の型 |
| `src/record/` | SQLite v2，v1 取り込み，保存先，`.ttrec` |
| `src/replay.rs` | 時刻の進行，1 打鍵送り，ログから求めるリプレイ状態 |
| `src/ui/` | ratatui の各画面，キーボード描画，アニメーションの補助関数 |
| `examples/gen_sample_records.rs` | 固定 seed の合成記録生成器 |

## テスト

```sh
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check
cargo run --locked --example gen_sample_records
cargo test --locked --manifest-path tools/gen-data/Cargo.toml
cargo clippy --all-targets --locked --manifest-path tools/gen-data/Cargo.toml -- -D warnings
```

結合テストはファイルごとではなく 2 つのテスト用実行ファイルにまとめています．`tests/all/main.rs` が `tests/all/` の下の各ファイルをモジュールとして宣言し，`tests/perf/main.rs` には画像生成の時間を測るテストを置いています．時間のテストは 1 本ずつ走り，上限の判定は release ビルドのときだけ行います．絞り込みは実行ファイルとモジュールのパスで指定します．

```sh
cargo test --locked --test all                     # 時間のテスト以外の結合テストすべて
cargo test --locked --test all menu_ui::           # 旧 1 ファイル分
cargo test --locked --test all menu_ui::menu_step  # そのモジュールのうち名前がこの文字列で始まるテスト
cargo test --locked --test all localized           # どのモジュールでも名前にこの文字列を含むテスト
cargo test --locked --test perf -- --nocapture     # 時間のテストと測定値の出力
```

結合テストのファイルを足すときは `tests/all/` に置き，`tests/all/main.rs` に `mod` 行を加えます．共有の補助関数は `crate::common` です．同じ実行ファイルのテストは 1 つのプロセスで並列に走るので，環境変数やカレントディレクトリを変えず，一時ディレクトリはテストごとに作ります．

単体テストは実装と同じモジュールに置いています．`tests/all/` の結合テストでは，ローマ字の指定例と性質，各訓練モード，ベンチの平行版と巡回，手計算した分析指標と縮小推定，分析タブ，CSV の絞り込み，キーの絞り込みと直近項目の除外，advance の編集指標，v1/v2 設定の優先関係，同梱・利用者 TSV，モード・キー・ローマ字・短文画面，時刻を注入した `.ttrec` リプレイ，サンプル記録，設定文書の既定値，SQLite の移行，レイアウト，入力の読み替え，Press・Release・Repeat，コメントを保つ書き戻し，CLI，全翻訳文言を確かめます．

レイアウト定義は `include_str!` で同梱し，各 `src/layout/defs/*.toml` の先頭コメントに一次情報の URL を記録しています．結合テストは `tests/all/common/mod.rs` を通して `App` を作り，必ず一時ホームと一時 DB を渡します．ホームディレクトリの取得は `src/main.rs` だけで行い，ライブラリの保存先解決には明示的なホームパスが必要です．テストは利用者の設定やセッション DB を読みません．

本体は `data/*.tsv.gz` を `include_bytes!` で組み込み，各データを 1 回だけ展開します．データ生成器は workspace の構成員ではなく，lindera，IPADIC，アーカイブ処理の依存を本体へ持ち込みません．再生成と点検は[出題データ](data.ja.md)を参照してください．小さいサンプル記録生成器は通常の example target で，本体と同じ依存だけを使います．
