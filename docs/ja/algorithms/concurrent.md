# Concurrent — チェンジオーバー遅延 / 比率付き複合

:gb: [English version](../../en/algorithms/concurrent.md)

## 参考文献

Catania, A. C. (1966). Concurrent performances: Reinforcement
interaction and response independence. *Journal of the Experimental
Analysis of Behavior*, 9(3), 253-263.
https://doi.org/10.1901/jeab.1966.9-253

Herrnstein, R. J. (1961). Relative and absolute strength of response
as a function of frequency of reinforcement. *JEAB*, 4(3), 267-272.
https://doi.org/10.1901/jeab.1961.4-267

## 数学的定義

`Concurrent` スケジュールは、`k >= 2` 個のコンポーネント・スケジュ
ールを別々の operandum 上で提示する。`event.operandum == key` の
反応は `components[key]` にルーティングされ、他のすべてのコンポ
ーネントは同じ `now` で tick を受け取る。

### チェンジオーバー遅延 (COD)

被験体が operandum A から B に切り替えたとき、新しい operandum 上
の強化は切り替え後 `cod` 時間単位の間、抑制される（暗黙に消費さ
れる）。タイマーは切り替え反応でアンカーし、その後の確定した
切り替えごとに再アンカーする。

### チェンジオーバー比率 (COR / FRCO)

`cor > 0` の場合、切り替えは新しい operandum 上で `cor` 回連続で
反応して初めて *チェンジオーバーとしてカウントされる*。COD タイマ
ーはその `cor` 番目の反応でのみアームされる。新しい operandum 上
の反応 `1..cor-1` はまだ「チェンジオーバー」ではないため、COD の
ゲート対象には **ならない**。

## 状態変数

| 名前 | 型 | 用途 |
|---|---|---|
| `components` | dict[str, Schedule] | operandum キーごとのコンポーネント・スケジュール。 |
| `cod` | float (const) | COD 持続時間 (`>= 0`; `0` で無効)。 |
| `cor` | int (const) | COR 閾値 (`>= 0`; `0` で無効)。 |
| `last_operandum` | str? | 最新の **確定** 反応の operandum。 |
| `switch_time` | float? | 最後の確定チェンジオーバーの時刻（COD 用）。 |
| `consecutive_new_count` | int | COR 連続中のランニング・カウント。 |
| `last_now` | float? | 単調時間チェック用。 |

## Step 擬似コード

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now

    if event is None:
        // 純 tick: 全コンポーネントを進める。チェンジオーバー・ロジックなし。
        outcome, _ = advance_components(now, None, None)
        return outcome

    operandum = event.operandum
    if operandum not in components:
        raise Config(...)

    outcome, from_event = advance_components(now, operandum, event)

    // ゲーティング前にチェンジオーバー状態を登録する。チェンジオーバー
    // 反応自体はそれが開く COD の対象となる。
    register_event(operandum, now)

    if outcome.reinforced and from_event and cod_active(operandum, now):
        // 強化を消費する。meta に抑制が記録される。
        return Outcome::unreinforced_with_meta({
            "cod_suppressed": true, "operandum": operandum,
        })
    return outcome
```

### `advance_components(now, event_operandum, event)`

マッチしたコンポーネント以外のすべてを `(now, None)` でステップし、
マッチしたものは `(now, event)` でステップする。`(outcome,
from_event)` を返す:

1. イベント・マッチしたコンポーネントの outcome が強化した場合
   → `from_event=True`。
2. そうでなければ、他のコンポーネントから最初に強化した tick
   outcome（挿入順）→ `from_event=False`。
3. そうでなければ、イベント・マッチしたコンポーネントの（強化なし）
   outcome をその `meta` を保持したまま → `from_event=True`。
4. そうでなければ空の `Outcome()` → `from_event=False`。

根拠: 純 tick で発火する時間ベース・コンポーネント (FT/VT/RT) は、
イベントが別の operandum に到着したからといって暗黙に破棄されて
はならない。

### `register_event(operandum, now)`

- 史上初の反応 (`last_operandum is None`): operandum を記録し、連続
  をリセット。チェンジオーバーではない。
- `operandum == last_operandum`: `consecutive_new_count = 0` にリ
  セット。
- `operandum != last_operandum`:
  - `cor == 0` の場合: `switch_time = now`、
    `last_operandum = operandum`、連続リセット。即時チェンジオーバー。
  - `cor > 0` の場合: `consecutive_new_count` をインクリメント。
    `cor` に達したときチェンジオーバーを確定する（`switch_time = now`、
    `last_operandum = operandum`、連続リセット）。

### `cod_active(operandum, now)`

次のすべてが成立するとき `True` を返す:

- `cod > 0`
- `switch_time is not None`
- `operandum == last_operandum`（ゲーティングは切り替え **先** の
  operandum にのみ適用される）
- `(now - switch_time) < cod - TIME_TOL`

## Reset セマンティクス

- 各コンポーネントに対して `component.reset()`。
- `last_operandum = None`
- `switch_time = None`
- `consecutive_new_count = 0`
- `last_now = None`

## エッジケース

- コンポーネントが `< 2` 個: `Config`。
- 負の `cod` または `cor`: `Config`。
- イベントに未知の operandum: `Config`。
- **ゲーティング順序**: チェンジオーバー反応はそれが開く COD
  ウィンドウに覆われる。新しい operandum に切り替えて同じ反応で
  強化を得た場合、その強化は抑制される。
- **tick 側の強化は決してゲートされない**。A 上の反応と B 上の
  タイマー発火強化が同じ `step()` 呼び出しに落ちた場合、B の強化
  は `from_event=False` で表面化され、COD ゲートされない。
- 同じ tick で複数の非イベント・コンポーネントが発火した場合、
  最初（挿入順）が勝ち、他は暗黙に破棄される。呼び出し側はこれ
  を避けるため十分な頻度でステップすべき。

## 方向別 COD

`cod_directional` で方向ごとの COD 上書きをサポートする。
`(from_operandum, to_operandum)` → 秒のマップで、マッチする
トランジションが発生したときに、そのスイッチに限り基本 `cod` を
方向別の値で置き換える。自己トランジションおよび負値は構築時に
拒否される（`Config` エラー）。マップに存在しない方向については
基本 `cod` が引き続き適用される。

Rust では `Concurrent` の `cod_directional: IndexMap<(String, String), f64>`
として実装される。Python では `dict[tuple[str, str], float]` のキーワード
引数として公開される。

## 成分形式の罰

`punish: dict[str, Schedule]`（Rust:
`IndexMap<String, Box<dyn Schedule>>`）は、強化判定後に step される
オペランダムごとの罰スケジュールを取り付ける。罰スケジュールは
メイン成分と COD 状態を **共有しない**。これらのサブスケジュール
が発行する負振幅の強化子（`label="SR-"`）は、返却される `Outcome`
にアクティブな強化子として現れる。1 回の step 内で強化と罰は相互
排他である。

## 決定性

コンポーネントの決定性を継承する。複合自体に RNG はない。

## Swift オリジナルとの差異

legacy Swift パッケージに並立スケジュールは存在しない。本複合は
両ポートで新規実装。
