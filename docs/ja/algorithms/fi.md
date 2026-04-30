# FI — 固定間隔 (Fixed Interval)

:gb: [English version](../../en/algorithms/fi.md)

## 参考文献

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

Catania, A. C., & Reynolds, G. S. (1968). A quantitative analysis of
the responding maintained by interval schedules of reinforcement.
*Journal of the Experimental Analysis of Behavior*, 11(3, Pt. 2),
327-383. https://doi.org/10.1901/jeab.1968.11-s327

## 数学的定義

構築時（または前回の強化子以降）から `interval` 時間単位以上経過
した後に最初に発せられた反応が強化される。間隔中の反応はクロックを
リセットせず、強化もされない。Tick (`event=None`) は決して強化
しない — スケジュールは反応を要求する。

判定基準（Swift オリジナル）: `numOfResponses > previousNumOfResponses
AND elapsed >= interval`。

## 状態変数

| 名前 | 型 | 用途 |
|---|---|---|
| `interval` | float (const) | 固定間隔長 (`> 0`)。 |
| `arm_time` | float | 次の反応が強化可能になる絶対時刻。`interval` に初期化。 |
| `last_now` | float? | 単調時間チェック用。 |

## Step 擬似コード

```
fn step(now, event):
    check_monotonic(now, last_now)
    check_event_time(now, event)
    last_now = now
    if event is None:
        return Outcome::unreinforced()
    if now + TIME_TOL >= arm_time:
        arm_time = now + interval
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

## Reset セマンティクス

- `arm_time = interval`
- `last_now = None`

## エッジケース

- `interval <= 0` は `Config` を送出。
- 初期値 `arm_time = interval` は、最初の間隔が **最初のステップで
  はなく** `t = 0` でアンカーされることを意味する。最初のステップが
  `now = 0` で到着する呼び出し側は、`interval` より前に反応を
  提示しない限り、最初のステップが `now = interval / 2` で到着する
  呼び出し側と同一に振る舞う。
- 境界等価性: `now == arm_time` の反応は強化される (`TIME_TOL` の
  余裕を伴う包含的比較)。

## LimitedHold フック

- `_arm_time` (read) — 現在アーム中の失効時刻。
- `_withdraw_and_rearm(now)` — 強化子を発生させずに
  `arm_time = now + interval` を設定する。`LimitedHold` がウィンドウ
  失効時に使用。

## 決定性

決定論的。RNG なし。

## Swift オリジナルとの差異

Swift は `ResponseEntity.milliseconds` フィールドから経過時間を
計算していた。本移植版は絶対 `arm_time` アンカーを所有するため、
経過計算は `now - last_reinforcement` の減算となる。`>` vs `>=`
の条件は `TIME_TOL` で余裕を持たせた包含的比較によって保持される。
