# LimitedHold — 有限利用可能性ラッパ

:gb: [English version](../../en/algorithms/limited-hold.md)

## 参考文献

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts. (第 5 章)

Nevin, J. A. (1974). Response strength in multiple schedules.
*Journal of the Experimental Analysis of Behavior*, 21(3), 389-408.
https://doi.org/10.1901/jeab.1974.21-389

## 数学的定義

間隔ファミリのスケジュール (FI, VI, RI) に対するデコレータで、各
強化機会に長さ `hold` の有限利用可能ウィンドウを追加する。被験体
が `[arm_time, arm_time + hold]` 内に反応できなかった場合、機会は
撤回される: 内側スケジュールは強化子を送出せずに、新しい間隔が
引かれて `now` でアーム直しされる。

内側スケジュールに要求されるフック（duck-typed）:

- `_arm_time: float` — 現在の機会が利用可能になる絶対時刻。
- `_withdraw_and_rearm(now: float)` — 新しい間隔を引き、強化子を
  送出せずに `now` でアンカーする。

## 状態変数

| 名前 | 型 | 用途 |
|---|---|---|
| `inner` | Schedule | ラップされた間隔スケジュール。 |
| `hold` | float (const) | 利用可能ウィンドウ (`> 0`)。 |
| `last_now` | float? | ラッパ上の単調時間チェック用。 |

他のすべての状態は `inner` に存在する。

## Step 擬似コード

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now

    // 必要に応じて失効: hold ウィンドウが閉じたなら以前アームされた機会を撤回
    if now > inner._arm_time + hold + TIME_TOL:
        inner._withdraw_and_rearm(now)

    if event is None:
        return Outcome::unreinforced()

    arm_time = inner._arm_time
    if now + TIME_TOL >= arm_time and now <= arm_time + hold + TIME_TOL:
        // sequence/cursor/rng を進めるため inner に委譲する
        return inner.step(now, event)

    return Outcome::unreinforced()
```

## Reset セマンティクス

- `inner.reset()`
- `last_now = None`

## エッジケース

- `hold <= 0` は `Config` を送出。
- Duck タイプチェック: `inner` が `_arm_time` または
  `_withdraw_and_rearm` を欠く場合、`Config` を送出。
- 失効は **すべて** のステップ（tick でも反応でも）で、イベントが
  ディスパッチされる前にチェックされる。ラッパが長時間のギャップ
  後にステップされた場合、ギャップは正しく複数の失効済みウィンドウ
  を閉じる — ただし `_withdraw_and_rearm` は次のウィンドウを `now`
  でアンカーするため、実際に再生成されるのは最新のウィンドウのみで
  あり、見逃したウィンドウ各々ではない。呼び出し側は、実験者の意図
  と整合するよう十分な頻度でステップすることが期待される。
- 境界等価性: `now >= arm_time` と `now <= arm_time + hold` は
  ともに `TIME_TOL` を伴う包含的比較。`now == arm_time + hold` の
  反応も強化される。
- ラッパは内側の強化決定を観測しない。イベントが内側に到達するか
  どうかをゲートするだけ。

## 決定性

内側の決定性を継承する。ラッパ自身は RNG を追加しない。

## Swift オリジナルとの差異

Swift ソースに LimitedHold は存在しない。Ferster & Skinner (1957)
第 5 章に記述される有限利用可能性意味論をモデル化するため、
`contingency-py` が新たに導入した構造である。
