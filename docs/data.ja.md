[English](data.md) | **日本語**

# 出題データ

rs-typing-trainer は，決定的に生成した4種類の gzip 圧縮 TSV を同梱します．`include_bytes!` で組み込み，起動時に1回だけ展開してキャッシュします．入力の SHA-256，生成日，フィルタ条件，出力の SHA-256，lindera と辞書の版は [`data/manifest.toml`](../data/manifest.toml) に記録しています．著作権表示と出典は [`data/NOTICE`](../data/NOTICE) にあります．

パブリックドメインの東雲フォント0.9.11も，12ドットと16ドットで同梱します．どちらも JIS X 0208 の全角6,879字，印字可能な ASCII 95字，JIS X 0201 の半角カナ63字を Unicode コードポイントで引けます．`data/NOTICE` には，UTF-8 に変換した `LICENSE` と `AUTHORS` の全文，配布元 URL を収録しています．

## 同梱ファイル

| ファイル | 行数 | 列 |
|---|---:|---|
| `data/en_words.tsv.gz` | 20,000 | `word`，`rank`，`tier` |
| `data/ja_words.tsv.gz` | 12,000 | `surface`，`reading`，`rank`，`tier` |
| `data/en_sentences.tsv.gz` | 19,696 | `text`，`tatoeba_id`，`tier` |
| `data/ja_sentences.tsv.gz` | 7,394 | `text`，`reading`，`source_id`，`author`，`tier` |
| `data/font/shinonome12.bin.gz` | 7,037字形 | 固定長バイナリレコード |
| `data/font/shinonome16.bin.gz` | 7,037字形 | 固定長バイナリレコード |

展開後のフォントは，20バイトのヘッダから始まります．内容は `SHNFONT\0`，形式版，ドットの高さ，半角・全角のピクセル幅，リトルエンディアンの字形数，予約4バイトです．続く固定長レコードはコードポイント順で，Unicode の `u32`，幅1バイト，高さ分の `u16` 行をすべてリトルエンディアンで格納します．x 座標の画素は `15-x` ビットです．gzip の時刻は0です．圧縮後の大きさは，12ドットが113,046バイト，16ドットが150,545バイトです．

読みは，ひらがなと `、。「」` からなります．長音記号は，出題文の表記にも長音記号がある場合だけ現れます．日本語短文の出典 ID は，`tools/gen-data/sources/aozora_selection.tsv` にある6桁の作品 ID です．`tier=core` は英単語2,000語，英文2,070文，人手で点検した日本語の単語561語と和文537文で，それ以外は `tier=ext` です．

## 出典とライセンス

- 英単語の頻度は，パブリックドメインの英語書籍30冊と Tatoeba の CC0 英文を合わせて数えています．SCOWL のレベル60までを使い，英米綴りに絞っています．SCOWL の著作権表示と許諾文の全文は `data/NOTICE` に残しています．
- 日本語の単語と短文は，著作権の切れた日本語作品560編から作っています．各作品の作品名，著者名，入力者名は `data/NOTICE` に残しています．
- 日本語の読みには lindera 6.2.0 と，組み込みの IPADIC `mecab-ipadic-2.7.0-20250920` 辞書を使っています．NAIST の許諾文と無保証条項，ICOT 条項を含む全文を `data/NOTICE` に残しています．
- 英語短文は Tatoeba の CC0 英文エクスポートから選んでいます．CC0 に表示義務はありませんが，`tatoeba.org` を出典として記載しています．
- 大きな出題文字の字形は，`http://openlab.ring.gr.jp/efont/shinonome/` の東雲フォント0.9.11です．配布物はフォントデータ・文書・スクリプトをパブリックドメインとして提供しています．条件と作者一覧の全文は `data/NOTICE` にあります．

パブリックドメインの扱いは法域によって異なる場合があります．異なる保護期間の地域で配布する場合は，原作品の状態を確認してください．

## 加工手順

英語書籍は `START OF` と `END OF` の行で本文を切り出し，配布用の前後部分を除きます．ASCII 英字を大文字・小文字を区別せずに数え，指定した SCOWL 語表との共通部分を取り，`tools/gen-data/blocklist_en.txt` を適用して頻度順に並べます．

日本語アーカイブは Shift_JIS から変換します．題名・著者，凡例，注記，ルビ記号，底本以降を本文から除きますが，ルビの親文字と読みは照合用に保持します．IPADIC の名詞，独立した動詞・形容詞，副詞を原形ごとに数えます．原形をもう一度解析し，各形態素の `reading` 列をつないで単語の読みを求めます．

短文候補は15〜45文字で，かぎ括弧の対応が取れ，指定した日本語文字と約物だけからなる文です．読みには IPADIC の `pronunciation` 列ではなく `reading` 列を使います．助詞の「は」「へ」「を」は表記を保ち，ルビ付きの位置ではルビを優先します．読みが `*` の未知語を含む候補は除外します．点検ファイルと override ファイルで人手修正を記録します．

CC0 の英文は30〜90文字の印字可能 ASCII に絞り，重複と除外語を含む文を落とし，条件を満たすすべての文を収録します．日本語は1作品あたり最大15文です．安定したマップ，決定的な同順位処理，FNV-1a 順により再生成結果を固定します．

## 固定データと日替わり出題

データ API には，ベンチマーク用の固定された全件と，練習用の日替わり出題の2つの入口があります．日替わり出題は，まず `include` / `only` を母集団へ適用し，ローカル日付 `YYYY-MM-DD` と DB の `meta` 表に初回だけ保存する32バイトの乱数の塩から決定的に選びます．

通常は頻度上位の核65%と裾野35%です．誤った項目は，最後の誤りから1・3・7・14日後に全体の最大10%まで戻します．短文は `item_exposure.count = 0` を優先し，その後は出題回数の少ない順です．水曜は数字・記号を含む文，金曜は日本語の1人の作家，日曜は裾野50%を優先します．現在のテーマはタイトルとモード選択に表示します．

日本語の単語または短文で読みが違う場合は `F2` を押します．その項目を飛ばし，次回以降は除外します．報告は設定した DB と同じディレクトリの `reading_reports.tsv` に `日時<TAB>kind<TAB>text<TAB>reading` で追記します．

## 再生成

生成器は本体と独立した Cargo プロジェクトです．必要なら入力一式を取得し，リポジトリのルートで次を実行します．

```sh
tools/gen-data/sources/fetch.sh /path/to/p4in
cargo run --release --manifest-path tools/gen-data/Cargo.toml -- \
  --input /path/to/p4in --extra-aozora /path/to/extra/aozora --out data/
cargo run --release --manifest-path tools/gen-data/Cargo.toml -- \
  --font-source /path/to/p4in/shinonome/shinonome-0.9.11 --out data/font \
  --font-notice data/NOTICE --font-manifest data/manifest.toml
```

フォントのコマンドは2番目に実行します．データのコマンドは `data/NOTICE` と `data/manifest.toml` を作り直し，フォントのコマンドがフォントの NOTICE，入力の SHA-256（配布物と，そこから読む各ファイル），フォントの出力を書き戻します．`fetch.sh` は配布物を SHA-256 `95663c95c92ba5765f63ccbdf033eb93b707be01812a989c548db943479c838f` と照合し，展開先の隣に配布物があればフォントのコマンドも同じ照合をします．両方のコマンドを2回実行しても `data/` は変わりません．

フォントファイルの形式，辞書キャッシュ，点検ファイル，override の形式，決定的な生成日の扱いは [`tools/gen-data/README.md`](../tools/gen-data/README.md) にあります．

## 利用者ファイル

設定で指定したファイルは，対応する同梱カテゴリだけを置き換えます．英語の単語・短文ファイルは，空でない1行を1問として読みます．日本語ファイルはタブ区切りの2列です．

```text
text<TAB>reading
```

`#` で始まる行と空行は無視します．日本語の読みは，ローマ字判定器で入力できる必要があります．
