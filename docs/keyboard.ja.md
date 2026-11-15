[English](keyboard.md) | **日本語**

# キーボードの配列と表示

## 同梱レイアウト

rs-typing-trainer は 7 つのレイアウトを同梱しています．

- QWERTY（US）: `qwerty`
- Dvorak（US）: `dvorak`
- Colemak: `colemak`
- Angle Mod なしの Colemak-DH: `colemak-dh`
- 公式 ANSI Angle(Z) 版 Colemak-DH: `colemak-dh-ansi`
- Workman: `workman`
- かな刻印を扱わない JIS QWERTY: `jis-qwerty`

`keyboard.geometry` は，レイアウトとは別に `ansi` または `jis` を指定します．物理配置はキーの段・幅・担当指を決め，レイアウトは各物理キーが出す文字を決めます．物理キー名には，`KeyA`，`Semicolon`，`Digit1`，`IntlYen`，`IntlRo` など，W3C の `KeyboardEvent.code` を使います．`Digit6` の既定担当は左人差し指です．別の一般的な割り当てを使う場合は，`digit6_finger = "right-index"` を指定します．

従来名の `colemak-dh` は，Angle Mod なしの下段 `zxcdvkh,./` を互換性のため維持します．公式 ANSI 版は Colemak-DH の [ANSI Angle(Z) 配置](https://colemakmods.github.io/mod-dh/keyboards.html)に従い，下段を `xcdvzkh,./` とします．左手下段を 1 キー左へ移し，`z` を中央のキーへ移す点が異なります．

画面上のキーボードは次のキーを強調し，Shift が必要な場合は Shift も強調します．ANSI と JIS の各段は実物の左端のずれを持ち，1 キー 3 列のコンパクト表示でも近似します．正打は約 150 ms 緑に光り，誤打は押したキーと正しいキーの両方が赤く光ります．`show = "dim"` では刻印を暗くし，`show = "hidden"` ではキーボードを描きません．`show = "auto"` は縮小推定した正確さが 97% 以上で，IKI が利用者の中央値以下のキーから薄くし，十分な記録が集まると刻印を消します．次に打つキーは常に残します．既定の初期値は `full` のままで，ベンチでは固定の full 表示を強制します．`finger_guide = false` で担当指の色帯を隠せます．

## 入力レイアウトの読み替え

通常は `input_layout = "same"` とし，OS から届いた文字をそのまま判定します．OS の配列と練習する配列が異なる場合だけ，入力レイアウトを明示します．

例えば，`input_layout = "qwerty"`，`layout = "dvorak"` のとき，入力した `d` は物理キー `KeyD` に引き直されます．Dvorak では同じキーが `e` を出すため，練習では `e` として判定します．Shift の状態も維持するため，`D` は `E` になります．入力レイアウトから引けない文字は，変更せずに通します．

## 利用者定義のレイアウト

`~/.config/rs-typing-trainer/layouts/` に `*.toml` を置きます．`--config PATH` を使う場合は，そのファイルと同じ場所の `layouts/` を読みます．形式は同梱定義と同じです．

```toml
name = "my-layout"

[keys.KeyA]
normal = "a"
shift = "A"

[keys.Semicolon]
normal = ";"
shift = ":"
```

通常時と Shift 時の値には，それぞれ Unicode スカラー値を 1 文字指定します．同じ文字を異なる 2 つの物理キーへ割り当てることはできません．重複がある場合は，ファイル名と文字を含むエラーを出し，そのレイアウト群を読み込みません．

## キーを離したイベント

起動時に crossterm でキーボード拡張への対応を確かめ，対応端末だけで `REPORT_EVENT_TYPES` を有効にします．これは kitty，WezTerm，Ghostty，foot などが実装する kitty キーボードプロトコルの機能です．端末名から推測せず，実行時の応答で判定します．

対応端末では，Release イベントを直前の同じ物理キーの押下へ結び付け，`release_us` を埋めます．この場合は `sessions.release_events` が true になります．Release と Repeat は，文字入力にも画面操作にも使いません．非対応端末では Press だけで動作し，`release_us` は空のまま，`sessions.release_events` は false になります．

キーボード拡張のフラグは，通常終了時にもパニック時の端末復元処理でも解除します．
