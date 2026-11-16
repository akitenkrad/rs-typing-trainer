[English](configuration.md) | **日本語**

# 設定リファレンス

`--config` を省略すると，`./config.toml`，`~/.config/rs-typing-trainer/config.toml` の順で最初に存在するファイルを読みます．どちらも無ければ，次の既定値を使います．`~/` で始まるパスは利用者のホームディレクトリを基準に展開します．コマンドラインの `--lang` は `general.lang` より優先され，`--graphics MODE` は `display.graphics` より優先されます．

対応するキーをすべて次の表にまとめます．「未設定」は，値を実行時に求めるか，同梱データ・既定の保存先を使うことを表します．`[game]`，`[characters]`，`[metrics]` は v1 互換キーです．新しい設定では v2 のキーを使ってください．

| キー | 型 | 既定値 | 説明 |
|---|---|---|---|
| `general.lang` | `en` / `ja` | `"en"` | 表示言語． |
| `display.prompt_style` | `large` / `normal` | `"large"` | 問題文の描き方．`large` は端末画像を使える場合は字ごとの画像タイル，それ以外は東雲の大きな字形（16ドット，入らなければ12ドット）で描き，どちらも入らなければ通常の文字にする．`normal` は端末の文字の大きさのまま，枠で囲んだカードに字間を広げて描く．モード選択画面の `Z` でも切り替えられる．旧キー `prompt_size` も読み込む（`normal` → `normal`，それ以外 → `large`）．アプリから保存すると `prompt_style` で書き戻す． |
| `display.feedback` | `summary` / `detailed` | `"summary"` | セッション後の気づき．`summary` は最も目立つ内容を 1 行，`detailed` は最大 3 行を示す．根拠が足りないときは値や文言を補わない． |
| `display.graphics` | `auto` / `kitty` / `sixel` / `iterm2` / `off` | `"auto"` | 端末画像の方式．`auto` は起動時に 1 回だけ端末へ問い合わせる．方式を明示しても，1 セルの画素数を取得できなければ使わない．標準入出力が端末でなければ常に `off`．tmux／screen 内の `auto` も passthrough を確かめられないため `off`．詳しくは[端末画像](graphics.ja.md)を参照． |
| `display.big_font` | `biz-ud-gothic` / `shinonome` / フォントのパス | `"biz-ud-gothic"` | 画像で描く出題の大きな文字に使う書体．`biz-ud-gothic` は同梱の BIZ UDゴシックの輪郭，`shinonome` は同梱のビットマップを整数倍に拡大して使う．それ以外の文字列は TTF・OTF・TTC（0 番目）のパスとして読み，`~/` はホームディレクトリを基準に展開する．ファイルを読めない・解析できない場合は BIZ UDゴシックへ戻し，理由を設定画面に表示する．端末画像を使えない場合，この設定は描画に影響しない． |
| `keyboard.layout` | 文字列 | `"qwerty"` | 同梱または利用者定義の練習レイアウト名． |
| `keyboard.input_layout` | 文字列 | `"same"` | 物理キーの読み替えに使う OS 側のレイアウト． |
| `keyboard.geometry` | `ansi` / `jis` | `"ansi"` | キーボードの物理配置． |
| `keyboard.show` | `full` / `dim` / `hidden` / `auto` | `"full"` | 画面上のキーボード表示と自動薄化． |
| `keyboard.style` | `keycap` / `raised` / `plain` | `"keycap"` | キーボードの見た目．場所が足りない場合は自動で簡素化． |
| `keyboard.heatmap` | `off` / `errors` / `latency` | `"off"` | 分析画面の初期熱分布． |
| `keyboard.finger_guide` | 真偽値 | `true` | 担当指の色帯を表示する． |
| `keyboard.digit6_finger` | `left-index` / `right-index` | `"left-index"` | `6` キーを担当する指． |
| `session.duration_seconds` | 整数 | `60` | 通常セッションの時間． |
| `session.item_timeout_seconds` | 整数 | `5` | `sentence`・`phrase`・`endurance` 以外で使う 1 項目の制限時間．0 で無効． |
| `session.error_mode` | `stop` / `advance` | `"stop"` | 誤打で止まるか，カーソルを進めるか． |
| `session.preview_items` | 整数 | `1` | 先読み項目数．0〜3． |
| `session.blind` | 真偽値 | `false` | 項目終了まで入力済み部分を隠す． |
| `session.retry_on_error` | 真偽値 | `false` | 誤打した項目を 1 回出し直す． |
| `session.result_input_delay_ms` | 整数 | `1500` | 終了直後の結果画面とベンチまとめ画面で，この時間（ms）だけ押下・リピート入力を無視する．0 で無効．10000 を超える値は 10000 に丸める． |
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
| `mode.phrase.lang` | `en` / `ja` | `"en"` | 句の言語．モード選択画面の `L` でも切り替える． |
| `mode.phrase.course` | `short` / `normal` / `long` / `mixed` | `"normal"` | 句の長さのコース．モード選択画面の `C` でも切り替える． |
| `data.words_en` | パス | `—` | 英単語 TSV の置換．未設定なら同梱データ． |
| `data.words_ja` | パス | `—` | 日本語単語 TSV の置換． |
| `data.phrases_en` | パス | `—` | 追加する英語句 TSV．同梱の句も残る． |
| `data.phrases_ja` | パス | `—` | 追加する日本語句 TSV．同梱の句も残る． |
| `data.sentences_en` | パス | `—` | 英文 TSV の置換． |
| `data.sentences_ja` | パス | `—` | 和文 TSV の置換． |
| `practice.daily_rotation` | 真偽値 | `true` | 決定的な日替わり出題を使う． |
| `practice.core_ratio` | 数値 | `0.65` | 日替わり出題に占める頻度上位群の割合． |
| `practice.weekly_theme` | 真偽値 | `true` | 曜日テーマを有効にする． |
| `practice.auto_difficulty` | 真偽値 | `true` | 直近 3 回の adaptive の正確さの中央値から，ペーサーとドリル割合を自動調節する． |
| `analysis.pause_ms` | 整数 | `2000` | 休止として扱う IKI の間隔． |
| `analysis.shrinkage_k` | 数値 | `10.0` | 遷移・キーの縮小推定の強さ． |
| `analysis.half_life_days` | 数値 | `14.0` | «最近» の遷移モデルに使う半減期（日）． |
| `analysis.min_transition_n` | 整数 | `5` | コストの高い遷移の候補に入れるために必要な生の観測数． |
| `analysis.spelling_suggestions` | 真偽値 | `true` | 本人向けのローマ字綴り案を有効にする． |
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

設定画面では，レイアウト，入力レイアウト，物理配置，キーボード表示，キーボードの見た目，出題文字の大きさ，端末画像，言語，誤入力の扱い，結果の気づきを変更し，コメントと関係のない TOML キーを保ちます．各値の動作は[訓練モード](modes.ja.md)，[訓練機能](training.ja.md)，[キーボードの配列](keyboard.ja.md)を参照してください．
