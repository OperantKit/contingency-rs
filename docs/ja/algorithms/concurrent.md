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

被験体が operandum A から B に切り替えたとき、切り替え後 `cod`
時間単位の間、新しい operandum 上の反応は強化されない。タイマー
は切り替え反応でアンカーし、その後の確定した切り替えごとに再
アンカーする。

COD の間に B のコンポーネントが設定した強化子は捨てられず、
**保持される**。COD が明けた後の B への最初の反応で渡され、B の
コンポーネントはその受け渡しまで次の間隔（または比率）を始めな
い。COD の間に B を離れても、強化子は B に残る。これは Herrnstein
(1961) の手続きである。切り替えから 1.5 秒以内の反応は強化され
ず、設定された強化子は取られるまで残っていた。
B の強化子を保持した step で別のコンポーネントが時間経過により強化
子を出し、罰が発火しない場合は、その強化子をそのまま返す。

### 固定比率切替 (FRCO)

`frco > 0` の場合、切り替えは新しい operandum 上で `frco` 回連続で
反応して初めて *チェンジオーバーとしてカウントされる*。COD タイマ
ーはその `frco` 番目の反応でのみアームされる。新しい operandum 上
の反応 `1..frco-1` はまだ「チェンジオーバー」ではないため、COD の
ゲート対象には **ならない**。

## 状態変数

| 名前 | 型 | 用途 |
|---|---|---|
| `components` | dict[str, Schedule] | operandum キーごとのコンポーネント・スケジュール。 |
| `cod` | float (const) | COD 持続時間 (`>= 0`; `0` で無効)。 |
| `frco` | int (const) | FRCO 閾値 (`>= 0`; `0` で無効)。 |
| `last_operandum` | str? | 最新の **確定** 反応の operandum。 |
| `switch_time` | float? | 最後の確定チェンジオーバーの時刻（COD 用）。 |
| `consecutive_new_count` | int | FRCO 連続中のランニング・カウント。 |
| `pending` | dict[str, list[Reinforcer]] | 複合側が operandum ごとに保持する強化子（古い順。FR/VR/RR/FI/VI/RI 以外のコンポーネント用）。 |
| `last_now` | float? | 単調時間チェック用。 |

## Step 擬似コード

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now

    if event is None:
        // 純 tick: 全コンポーネントを進める。チェンジオーバー・ロジックなし。
        outcome, _, _ = advance_components(now, None, None, gated=false)
        return outcome

    operandum = event.operandum
    if operandum not in components:
        raise Config(...)

    // ゲーティング前にチェンジオーバー状態を登録する。チェンジオーバー
    // 反応自体はそれが開く COD の対象となる。
    register_event(operandum, now)
    gated = cod_active(operandum, now)

    outcome, from_event, held = advance_components(now, operandum, event, gated)
    if not gated:
        outcome, from_event = deliver_pending(operandum, now, outcome, from_event)

    if held:
        // 強化子はコンポーネントに設定されたまま残る。別のコンポー
        // ネントが時間経過で出した強化子はそのまま返す。
        if outcome.reinforced and not from_event:
            return outcome with meta["cod_suppressed"] = true
        // それ以外は、この反応が COD のために強化されなかったことを
        // meta に記録する。
        return Outcome::unreinforced_with_meta({
            "cod_suppressed": true, "operandum": operandum,
        })
    return outcome
```

### `advance_components(now, event_operandum, event, gated)`

マッチしたコンポーネント以外のすべてを `(now, None)` でステップし、
マッチしたものは `(now, event)` でステップする。`gated` のときは、
マッチしたものを代わりに `step_gated` でステップする。`(outcome,
from_event, held)` を返す:

1. イベント・マッチしたコンポーネントの outcome が強化した場合
   → `from_event=True`。
2. そうでなければ、他のコンポーネントから最初に強化した tick
   outcome（挿入順）→ `from_event=False`。
3. そうでなければ、イベント・マッチしたコンポーネントの（強化なし）
   outcome をその `meta` を保持したまま → `from_event=True`。
4. そうでなければ空の `Outcome()` → `from_event=False`。

`held` は、`step_gated` がマッチしたコンポーネントに強化子を保持
したときに限り `true`。

根拠: 純 tick で発火する時間ベース・コンポーネント (FT/VT/RT) は、
イベントが別の operandum に到着したからといって暗黙に破棄されて
はならない。

### `step_gated(component, now, event)`

```
if component is FR | VR | RR | FI | VI | RI:
    probe = deep_copy(component)
    if probe.step(now, event).reinforced:
        component.step(now, None)  // tick のみ: 強化子は設定されたまま
        return Outcome(), held=true
    return component.step(now, event), held=false

out = component.step(now, event)
if out.reinforced:
    pending[event.operandum].push(out.reinforcer)
    return Outcome(), held=true
return out, held=false
```

比率・時隔スケジュールでは、反応をまず複製で試す。強化するはずなら、
本物のコンポーネントには時間の tick だけを渡す。そのため時隔スケ
ジュールはアームされたまま、比率スケジュールは要求数の 1 反応手前
のまま残り、COD 明けの最初の反応がそれを満たし、次の間隔や比率は
そこから始まる。強化しない反応は本物のコンポーネントに適用するの
で、COD 中の反応も比率の数に入る。

それ以外のコンポーネント（FT/VT/RT、DRO/DRL/DRH、入れ子の複合）
は tick で強化したり、すべての反応を見る必要があったりするので、
反応を渡さないと強化子が失われるか、コンポーネントの状態が狂う。
そこで反応は通常どおり適用し、出た強化子を `pending` に積む。コン
ポーネント自身の次の周期はすぐに始まる。

Rust 版では型の判定に `Schedule::probe_response` を使う。FR・VR・RR・
FI・VI・RI は自分の `clone` を step して `Some(強化するか)` を返し、
それ以外のスケジュール（`LimitedHold` と利用者定義のスケジュールを
含む）は既定の `None` を返して待ち行列の経路に入る。`pending` は
`IndexMap<String, VecDeque<Reinforcer>>` である。

### `deliver_pending(operandum, now, outcome, from_event)`

ゲートされていない反応について `advance_components` の後に呼ぶ。
`pending[operandum]` が空なら入力をそのまま返す。そうでなければ、
この反応でコンポーネントが出した強化子を列の末尾に積み、最も古い
ものを `time = now`（`from_event = true`）で渡す。1 反応で渡す強化
子は高々 1 つである。`reset()` は `pending` を空にする。

### `register_event(operandum, now)`

- 史上初の反応 (`last_operandum is None`): operandum を記録し、連続
  をリセット。チェンジオーバーではない。
- `operandum == last_operandum`: `consecutive_new_count = 0` にリ
  セット。
- `operandum != last_operandum`:
  - `frco == 0` の場合: `switch_time = now`、
    `last_operandum = operandum`、連続リセット。即時チェンジオーバー。
  - `frco > 0` の場合: `consecutive_new_count` をインクリメント。
    `frco` に達したときチェンジオーバーを確定する（`switch_time = now`、
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
- `pending = {}`

## エッジケース

- コンポーネントが `< 2` 個: `Config`。
- 負の `cod` または `frco`: `Config`。
- イベントに未知の operandum: `Config`。
- **ゲーティング順序**: チェンジオーバー反応はそれが開く COD
  ウィンドウに覆われる。新しい operandum に切り替えて同じ反応で
  強化子が設定された場合、その強化子は COD 明けの最初の反応まで
  保持される。
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
`step_gated` の probe は複製なので、捨てた probe は本物のコンポー
ネントの RNG を進めない。

## Swift オリジナルとの差異

legacy Swift パッケージに並立スケジュールは存在しない。本複合は
両ポートで新規実装。
