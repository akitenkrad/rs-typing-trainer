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

`PRAGMA user_version` は 3 です．v1 の `sessions` 表を検出すると，表を `sessions_v1` に改名して残します．各行は 1 回だけ `mode = random`，`metrics_version = 0` として写し，元の JSON を `legacy_payload` に保存します．net WPM と CPS は payload の `correct_chars` とナノ秒精度の `total_duration` から再計算します．v1 には raw WPM の計算に必要な情報が無いため，raw WPM は `NULL` にします．従来の v2 スキーマで作られた DB も，開いたときに冪等に更新します．v1 の行は履歴で表示できますが，打鍵ログとして export できません．

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
