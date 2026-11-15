[English](replay.md) | **日本語**

# セッション記録のリプレイ

`replay` は，記録した打鍵を通常のセッション処理へ流し，出題の進み方・カーソル・誤打表示・次のキーの強調・指の色帯・時間・WPM を含む通常のプレイ画面を描きます．セッション DB や利用者の設定は開きません．可搬記録の形式は[記録形式と指標](records.ja.md)を参照してください．

```sh
rs-typing-trainer replay examples/records/dvorak-sentence-en.ttrec
```

## オプション

| オプション | 動作 |
|---|---|
| `--interval <MS>` | 記録した打鍵間隔を，一定のミリ秒値で置き換える． |
| `--speed <X>` | 記録時刻を `X` 倍速で再生する．既定は `1.0`．`--interval` と同時には使えない． |
| `--paused` | 一時停止で始める． |
| `--exit-on-end` | 最後の打鍵を適用したら終了する．省略時は最後の出題を打ち終えたプレイ画面を表示したままにする． |
| `--print` | TUI を開かず，モード・レイアウト・出題・件数・時間・指標を表示する． |

表示言語には共通オプションを使います．

```sh
rs-typing-trainer --lang ja replay examples/records/dvorak-words-en.ttrec --speed 2
rs-typing-trainer replay examples/records/qwerty-romaji-ja.ttrec --print
```

再生中は `Space` で再生・停止，`→` で 1 打鍵進み，`Esc`・`Q`・`Ctrl-C` で終了します．一時停止中も 1 打鍵ずつ進められます．リプレイの進行と操作は 1 行に表示します．最後まで進むと，最後の出題とキーボードを残したまま，その行を最終 WPM と正確さへ切り替えます．再生中と最後の指標は，通常プレイと同じログ基準の指標関数で求めます．リプレイ専用の採点処理は持ちません．

## サンプル記録

[`examples/records/`](../examples/records/) の記録は，[`examples/gen_sample_records.rs`](../examples/gen_sample_records.rs) で決定的に作る合成デモです．先頭ヘッダに合成記録であることを明記しています．生成器は同梱出題を選び，固定 seed，中央値 150〜200 ms の対数正規分布，物理的な遷移クラスによる速度差，少数の誤打と修正を使います．

```sh
cargo run --locked --example gen_sample_records
```

## 動画の録画

[asciinema](https://asciinema.org) の例:

```sh
asciinema rec -c "rs-typing-trainer replay examples/records/dvorak-sentence-en.ttrec --exit-on-end" typing.cast
```

[VHS](https://github.com/charmbracelet/vhs) では [`examples/demo.tape`](../examples/demo.tape) のような tape を使います．rs-games の一覧用録画では `--exit-on-end` を付けず，再生時間に 3.5 秒を加えて待ち，GIF の最後に打ち終えたプレイ画面を残します．
