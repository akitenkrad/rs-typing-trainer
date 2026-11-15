[English](configuration.md) | **日本語**

# 設定リファレンス

`--config` を省略すると，`./config.toml`，`~/.config/rs-typing-trainer/config.toml` の順で最初に存在するファイルを読みます．どちらも無ければ，次の既定値を使います．`~/` で始まるパスは利用者のホームディレクトリを基準に展開します．コマンドラインの `--lang` は `general.lang` より優先されます．

対応するキーをすべて次の表にまとめます．「未設定」は，値を実行時に求めるか，同梱データ・既定の保存先を使うことを表します．`[game]`，`[characters]`，`[metrics]` は v1 互換キーです．新しい設定では v2 のキーを使ってください．

| キー | 型 | 既定値 | 説明 |
|---|---|---|---|
| `general.lang` | `en` / `ja` | `"en"` | 表示言語． |
| `display.prompt_style` | `large` / `normal` | `"large"` | 問題文の描き方．`large` は大きな字形（16ドット，入らなければ12ドット）で描き，どちらも入らなければ通常の文字にする．`normal` は端末の文字の大きさのまま，枠で囲んだカードに字間を広げて描く．モード選択画面の `Z` でも切り替えられる．旧キー `prompt_size` も読み込む（`normal` → `normal`，それ以外 → `large`）．アプリから保存すると `prompt_style` で書き戻す． |
| `keyboard.layout` | 文字列 | `"qwerty"` | 同梱または利用者定義の練習レイアウト名． |
| `keyboard.input_layout` | 文字列 | `"same"` | 物理キーの読み替えに使う OS 側のレイアウト． |
| `keyboard.geometry` | `ansi` / `jis` | `"ansi"` | キーボードの物理配置． |
| `keyboard.show` | `full` / `dim` / `hidden` / `auto` | `"full"` | 画面上のキーボード表示と自動薄化． |
| `keyboard.style` | `keycap` / `raised` / `plain` | `"keycap"` | キーボードの見た目．場所が足りない場合は自動で簡素化． |
| `keyboard.heatmap` | `off` / `errors` / `latency` | `"off"` | 分析画面の初期熱分布． |
| `keyboard.finger_guide` | 真偽値 | `true` | 担当指の色帯を表示する． |
| `keyboard.digit6_finger` | `left-index` / `right-index` | `"left-index"` | `6` キーを担当する指． |
| `session.duration_seconds` | 整数 | `60` | 通常セッションの時間． |
| `session.item_timeout_seconds` | 整数 | `5` | 1 項目の制限時間．0 で無効． |
| `session.error_mode` | `stop` / `advance` | `"stop"` | 誤打で止まるか，カーソルを進めるか． |
| `session.preview_items` | 整数 | `1` | 先読み項目数．0〜3． |
| `session.blind` | 真偽値 | `false` | 項目終了まで入力済み部分を隠す． |
| `session.retry_on_error` | 真偽値 | `false` | 誤打した項目を 1 回出し直す． |
| `mode.kind` | モード名 | `"random"` | 起動時の練習モード． |
| `mode.keys` | 文字列 | `"bmwvz"` | 絞り込み・生成に使う文字． |
| `mode.key_filter` | `include` / `only` | `"include"` | 出題を絞り込む方法． |
| `mode.base_keys` | 文字列 | `"home"` | `only` に追加するキー．`none`，`home`，または文字列． |
| `mode.random.word_length` | 整数 | `3` | 擬似語の長さ． |
| `mode.random.allow_repeat` | 真偽値 | `true` | 擬似語内で同じ文字の反復を許す． |
| `mode.random.allow_consecutive_same_word` | 真偽値 | `false` | 同じ擬似語の連続出題を許す． |
| `mode.ngram.n` | 整数 | `2` | n-gram の長さ． |
| `mode.ngram.source` | `pairs` / `corpus` | `"pairs"` | 直積または同梱データ中の n-gram． |
| `mode.romaji.accept_apostrophe` | 真偽値 | `true` | んの `n'` を受理する． |
| `mode.romaji.accept_xn` | 真偽値 | `true` | んの `xn` を受理する． |
| `mode.sentence.lang` | `en` / `ja` | `"en"` | 短文の言語． |
| `mode.sentence.hide_typed` | 真偽値 | `false` | 入力済みの短文接頭部を隠す． |
| `data.words_en` | パス | `—` | 英単語 TSV の置換．未設定なら同梱データ． |
| `data.words_ja` | パス | `—` | 日本語単語 TSV の置換． |
| `data.sentences_en` | パス | `—` | 英文 TSV の置換． |
| `data.sentences_ja` | パス | `—` | 和文 TSV の置換． |
| `practice.daily_rotation` | 真偽値 | `true` | 決定的な日替わり出題を使う． |
| `practice.core_ratio` | 数値 | `0.65` | 日替わり出題に占める頻度上位群の割合． |
| `practice.weekly_theme` | 真偽値 | `true` | 曜日テーマを有効にする． |
| `analysis.pause_ms` | 整数 | `2000` | 休止として扱う IKI の間隔． |
| `analysis.shrinkage_k` | 数値 | `10.0` | 遷移・キーの縮小推定の強さ． |
| `adaptive.error_weight` | 数値 | `—` | 誤打のコスト（ms）．未設定なら IKI から求める． |
| `adaptive.drill_ratio` | 数値 | `0.3` | 生成ドリルの割合． |
| `adaptive.lang` | `en` / `ja` | `"en"` | 適応練習の言語． |
| `adaptive.max_share` | 数値 | `0.4` | 1 重点遷移が占める最大割合． |
| `pacer.enabled` | 真偽値 | `false` | 起動時からペーサーを有効にする． |
| `pacer.ratio` | 数値 | `1.07` | ベンチまたは直近中央値へ掛ける倍率． |
| `pacer.min_wpm` | 数値 | `20.0` | 目標の下限と，履歴が無いときの目標． |
| `unlock.order` | `frequency` / `rows` | `"frequency"` | キーを解放する順序． |
| `unlock.target_wpm` | 数値 | `35.0` | 解放条件に使う速度． |
| `gate.min_accuracy` | 数値 | `0.0` | 正確さの関門．0 で無効． |
| `rhythm.bpm` | 数値 | `—` | メトロノームの速さ．未設定なら IKI から求める． |
| `rhythm.bell` | 真偽値 | `false` | 拍で端末ベルを送る． |
| `burst.words` | 整数 | `12` | 1 本の語数．10〜15． |
| `endurance.minutes` | 整数 | `4` | 持久練習の時間．3〜5 分． |
| `ui.animation_enabled` | 真偽値 | `true` | 項目のスライドを有効にする． |
| `ui.animation_speed` | 数値 | `1.0` | アニメーション速度の倍率． |
| `ui.theme` | `default` / `mono` / `neon` | `"default"` | 配色テーマ． |
| `ui.show_next_preview` | 真偽値 | `true` | 次の項目を表示する． |
| `storage.db_path` | パス | `—` | セッション DB の保存先． |
| `game.duration_seconds` | 整数 | `60` | v1 の `session.duration_seconds`． |
| `game.word_timeout_seconds` | 整数 | `5` | v1 の `session.item_timeout_seconds`． |
| `game.word_length` | 整数 | `3` | v1 の `mode.random.word_length`． |
| `characters.charset` | 文字列 | `"abcdefghijklmnopqrstuvwxyz"` | v1 の文字集合．`mode.keys` へ読み替える． |
| `characters.allow_repeat` | 真偽値 | `true` | v1 の `mode.random.allow_repeat`． |
| `characters.allow_consecutive_same_word` | 真偽値 | `false` | v1 の random モード用設定． |
| `metrics.db_path` | パス | `—` | v1 の DB 保存先．`storage.db_path` が優先． |

設定画面では，レイアウト，入力レイアウト，物理配置，キーボード表示，キーボードの見た目，出題文字の大きさ，言語，誤入力の扱いを変更し，コメントと関係のない TOML キーを保ちます．各値の動作は[訓練モード](modes.ja.md)，[訓練機能](training.ja.md)，[キーボードの配列](keyboard.ja.md)を参照してください．
