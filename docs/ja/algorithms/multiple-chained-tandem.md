# Multiple / Chained / Tandem — 系列複合

:gb: [English version](../../en/algorithms/multiple-chained-tandem.md)

## 参考文献

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

Kelleher, R. T., & Gollub, L. R. (1962). A review of positive
conditioned reinforcement. *JEAB*, 5(4 Suppl), 543-597.
https://doi.org/10.1901/jeab.1962.5-s543

Reynolds, G. S. (1961). Behavioral contrast. *JEAB*, 4(1), 57-71.
https://doi.org/10.1901/jeab.1961.4-57

## 共通定義

3 つすべてが `N >= 2` 個のコンポーネント・スケジュールを合成し、
同じ `Schedule` プロトコルを公開する。活性コンポーネントが強化
したときの挙動が異なる。

**非活性コンポーネントはステップされない**。`step()` 呼び出しは
活性コンポーネントのみが受け取る。したがって時間ベース内側
スケジュールは **活性化後の最初のステップ** でアンカーする。これは
リンク遷移時に S^D とクロックがともに再スタートする実際のオペ
ラント箱と一致する。

## `Multiple` (`mult`)

コンポーネントを循環的に回転し、それぞれが独自の S^D を持つ。活性
コンポーネント上での強化は一次強化子を供給し、活性インデックスを
進める（最後の次は 0 にラップする）。

### 状態

| 名前 | 型 |
|---|---|
| `components` | list[Schedule] |
| `stimuli` | list[str] (一意) |
| `active` | int (活性インデックス) |
| `last_now` | float? |

### Step 擬似コード

```
fn step(now, event):
    // 単調 + イベント検証
    inner = components[active].step(now, event)
    stim = stimuli[active]
    meta = dict(inner.meta)
    meta["current_component"] = stim
    if inner.reinforced:
        active = (active + 1) mod N
        return Outcome::reinforced_with_meta(inner.reinforcer, meta)
    return Outcome::unreinforced_with_meta(meta)
```

### Reset

- `active = 0`
- すべてのコンポーネントをリセット。
- `last_now = None`

## `Chained` (`chain`)

異なる S^D を持つコンポーネントを通る系列チェイン。非終端の完了は
**条件性** 強化である: S^D は変わるが、一次 `Reinforcer` は供給
**されない**。終端（最後の）リンクのみが一次強化を供給する。
その後 `active` は 0 に戻る。

### Step 擬似コード

```
fn step(now, event):
    inner = components[active].step(now, event)
    if inner.reinforced:
        if active == N - 1:        // 終端
            active = 0
            meta = dict(inner.meta)
            meta["current_component"] = stimuli[active]
            return Outcome::reinforced_with_meta(inner.reinforcer, meta)
        // 非終端: 条件性強化
        active += 1
        meta = dict(inner.meta)
        meta["current_component"] = stimuli[active]
        meta["chain_transition"] = true
        return Outcome::unreinforced_with_meta(meta)
    meta = dict(inner.meta)
    meta["current_component"] = stimuli[active]
    return Outcome::unreinforced_with_meta(meta)
```

### Reset

`Multiple` と同じ。

## `Tandem` (`tand`)

構造的には `Chained` と同一だが、区別的な S^D を持たない。被験体は
リンク遷移で外部手がかりを受け取らない。`meta` は刺激ラベルでは
なく **整数** の `current_component` インデックスを運ぶ。

### Step 擬似コード

`meta["current_component"] = active`（整数インデックス）である
ことを除き、`Chained` と同一。

## エッジケース（3 つすべて）

- コンポーネントが `< 2` 個: `Config`。
- 刺激ラベルは一意で正しい長さでなければならない（Multiple /
  Chained）。
- `None` イベントは依然としてラッパの時間記録を進めるが、強化する
  かどうかは活性コンポーネントが決定する。
- `meta["chain_transition"]` は `Chained` / `Tandem` の非終端完了
  時に設定される — リンク遷移を記録するセッションに有用。

## 決定性

コンポーネントの決定性を継承する。複合自体に RNG はない。

## Swift オリジナルとの差異

legacy Swift パッケージに系列複合は存在しない。Multiple、Chained、
Tandem は `contingency-py` で新規。
