[English](records.md) | **日本語**

# 記録形式と指標

## SQLite v2

データベースは 3 表で構成します．完了した 1 セッションとすべての子行は，1 トランザクションで書き込みます．`sessions` の集計値は履歴の並べ替えを速くするために使い，`metrics_version = 2` の指標では `items` と `keystrokes` を正とします．

### `sessions`

| 列 | 意味 |
|---|---|
| `id` | 整数の主キー |
| `started_at`, `finished_at` | RFC 3339 の開始・終了日時 |
| `duration_ms` | セッション時間 |
| `mode`, `mode_params`, `bench_id` | モード名，JSON の条件，任意のベンチ ID |
| `layout`, `input_layout`, `geometry` | 設定した練習レイアウト・入力レイアウト・物理配置 |
| `error_mode` | `stop` または `advance` |
| `release_events` | 端末からキーを離したイベントを取得できたか |
| `app_version`, `metrics_version` | 書き込んだアプリと指標定義の版 |
| `net_wpm`, `raw_wpm`, `accuracy`, `iki_median_ms`, `iki_cv` | 主な指標の集計値 |
| `cps`, `consistency`, `max_combo` | 結果・履歴用の集計値 |
| `completed_words`, `timeout_words`, `mistypes_total` | 項目結果の集計値 |
| `legacy_payload` | v1 の `GameResult` JSON．v2 の記録では `NULL` |

### `items`

主キーは (`session_id`, `idx`) です．各行に `text`，任意の `reading`，`shown_us`，任意の `done_us`，`outcome`（`completed`・`timeout`・`aborted`）を保存します．µs の時刻はカウントダウン終了時を 0 とします．

### `keystrokes`

主キーは (`session_id`, `seq`) です．各行に `t_us`，任意の `release_us`，`item_idx`，書記素単位の `pos`，`kind`（`char`・`backspace`），任意の `typed` と `expected`，`correct`，任意の物理キー `code` を保存します．`code` は設定した入力レイアウトから推定し，文字を物理キーへ引けない場合だけ `NULL` にします．端末が Release イベントに対応している場合は `release_us` を埋めます．項目の切り替え中に捨てた入力は保存しません．

現在の `PRAGMA user_version` は 8 で，版 8 で[練習メニューの記録](#練習メニューの記録版-8)を足しました．v1 の `sessions` 表を検出すると，表を `sessions_v1` に改名して残します．各行は 1 回だけ `mode = random`，`metrics_version = 0` として写し，元の JSON を `legacy_payload` に保存します．net WPM と CPS は payload の `correct_chars` とナノ秒精度の `total_duration` から再計算します．v1 には raw WPM の計算に必要な情報が無いため，raw WPM は `NULL` にします．従来の v2 スキーマで作られた DB も，開いたときに冪等に更新します．v1 の行は履歴で表示できますが，打鍵ログとして export できません．

## 練習メニューの記録（版 8）

データベースの版 8 では，[練習メニュー](training.ja.md#練習メニュー)のための 3 表を足しました．古いデータベースを開くとこれらの表を作り，既存の行は変えません．既存の `prescriptions` と `review_schedule` も変えません．メニューの回を消すと，その段とスキルの記録も消えます（`ON DELETE CASCADE`）．

### `menu_runs`

実際に始めたメニュー 1 回につき 1 行です（`stats --menu` だけでは何も書きません）．

| 列 | 意味 |
|---|---|
| `id` | 整数の主キー |
| `date` | メニューの日（ローカルの日付） |
| `layout` | 練習レイアウト |
| `lang` | 設定した練習する言語を並べ替えてカンマでつないだキー（`en`・`ja`・`en,ja`）．段に実際に現れた言語ではありません |
| `catalog_version` | カタログの版．`menu-v1` |
| `minutes` | 指定した長さ．10・15・20 |
| `plan_json` | 組み立てたメニューを表示どおりに保存したもの．`stats --menu --json` と同じ JSON |
| `completed_steps` | 記録した段の数 |

日付・レイアウト・言語・カタログの版・長さが同じなら同じ日の回とみなすので，メニューを開き直しても同じ回の続きになります．まだ段を 1 つも記録していない間は，段を始めたときの内容で前の内容を置き換えます．

### `menu_steps`

主キーは (`run_id`, `idx`) で，`idx` は 1 から数えます．終えた段ごとに，`role`（`warmup`・`primary-a`・`primary-b`・`check`・`finish`），`prescription_id`（固定の段は `fixed.*`），`level`，保存したセッションの `session_id`，`metric` と `value`（処方の段の卒業の条件に使う値．測れないときは `NULL` で，0 にはしません），`advance_met`（`1`・`0`，判定できないときは `NULL`），`level_after` を保存します．段は 1 回のメニューにつき 1 度だけ記録し，もう一度遊んでも行は増えません．

### `skill_snapshots`

主キーは (`run_id`, `lang`, `component`, `source`) です．各行に，メニューの日 `date`，系列の言語 `lang`，`component`（`speed`・`accuracy`・`rhythm`・`transition`・`preview`・`recovery`・`overlap`），`raw_value`，観測数 `n`，`source`（`check` か `check+steps`），`corrected`（速さが難しさを補正した WPM なら `1`）を保存します．保存するのは生の値だけで，段と連続値は表示のたびに計算します．この表は打鍵ログから求めた値なので，`stats --rebuild-skills` で作り直せます．[スキルの推移](analysis.ja.md#スキルの推移)を参照してください．

### `meta` の処方の状態

処方ごとの現在の状態は，`meta` の `menu.state.<layout>.<lang>.<prescription_id>` に JSON で保存します．`level`，`consecutive`（条件を続けて満たした回数），`judgment`（`none`・`improved`・`no-change`・`ineffective`・`maintenance`），`paused_until`，`maintenance_due`，`started_on`，`pair_judged_run`（処方の段と確認段を最後に一緒に判定した回），`r3_counted`（数え済みの R3 の判定）です．この状態は打鍵ログから作り直せないため，`prescriptions` と同じく本体の記録として扱います．

### セッションの引数

メニューの段として遊んだセッションは，`mode_params` に `menu_run_id`，`menu_prescription_id`，`menu_step`（1 から数える）を足します．セッションからメニューへ辿れます．

## `.ttrec`

`.ttrec` は JSON Lines です．1 行目は `"type":"session"`，`format_version: 1`，正確な `duration_us`，`sessions` の各フィールドを持ちます．`duration_us` が無い従来の v1 ファイルも読み込め，その場合は `duration_ms` から復元します．以降は `"type":"item"` または `"type":"key"` と対応する行のフィールドを持ち，`shown_us` または `t_us` の順で並びます．標準出力またはファイルへ次のように書き出します．

```sh
rs-typing-trainer export SESSION_ID
rs-typing-trainer export SESSION_ID -o session.ttrec
```

DB を開かずにファイルを再生できます．

```sh
rs-typing-trainer replay session.ttrec
```

操作，要約，サンプル記録，動画の録画は[リプレイ](replay.ja.md)を参照してください．

## 指標の定義

- net WPM: `(正しく確定した書記素数 ÷ 5) ÷ 分`．
- raw WPM: `(文字の打鍵数 ÷ 5) ÷ 分`．Backspace は除きます．
- CPS: 1 秒あたりに正しく確定した書記素数．
- 正確さ: `stop` では正しい文字打鍵数 ÷ 全文字打鍵数，`advance` では編集後の最終バッファに残った正しい位置 ÷ 全位置．どちらも Backspace は分母から除きます．
- 打鍵間隔（IKI）: 同じ項目内で隣り合う打鍵の押下時刻の差．最初の打鍵は反応時間へ分け，2 秒を超える差は休止として除きます．
- IKI 中央値と CV: IKI の中央値と，母標準偏差 ÷ 平均．
- 安定度: IKI が 2 件以上なら `clamp(1 - IKI CV, 0, 1)`，それ以外は `0`．
- 反応時間: `shown_us` から項目の最初の記録打鍵まで．打鍵が有る項目から平均と p50・p90・p99 を計算します．
- 最大連続正解: 完了項目の最長の連続数．時間切れまたは誤打で 0 に戻り，誤打後に完了した項目を 1 として再開します．

百分位数は，並べ替えた隣接標本間を線形補間して求めます．
