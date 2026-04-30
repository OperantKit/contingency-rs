# Schedule Protocol — 言語非依存の契約

:gb: [English version](../en/protocol.md)

本ドキュメントは `contingency-rs` が実装する権威的な表面仕様である。
ファミリ内のすべてのスケジュールがこれに準拠し、複合スケジュールは
この上に構成される。

## 真実の源泉

`contingency-py` が executable specification の正典である。
`contingency-py/conformance/` 配下の 22 個の conformance fixture が
決定的スケジュールに対する bit 等価性のオラクルである。Rust crate は
同じ契約を再実装し、その fixture により検証される。

Python 側の定義：

- `contingency-py/src/contingency/interfaces.py` — `Schedule` Protocol。
- `contingency-py/src/contingency/entities.py` — 値オブジェクト。
- `contingency-py/src/contingency/errors.py` — エラー分類体系。

Rust 側のミラー（本 crate の navigation ポインタ）：

- [src/schedule.rs](../../src/schedule.rs) — `Schedule` トレイト。
- [src/types.rs](../../src/types.rs) — 値オブジェクト。
- [src/errors.rs](../../src/errors.rs) — エラー分類体系。

## Schedule インターフェース

### Rust

```rust
pub trait Schedule {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome>;
    fn reset(&mut self);
}
```

スケジュールは状態を持つため、可変借用が必要。内部可変性を必要とする
実装者（例: `Box<dyn Schedule>` を保持するラッパ）は trait レベルで
`&mut self` を使用し、RNG / カウンタを private フィールドに保持する。
内部値へ再ディスパッチする専用の `Box<dyn Schedule>` 実装が提供される。

### Python（正典）

```python
@runtime_checkable
class Schedule(Protocol):
    def step(self, now: float, event: ResponseEvent | None = None) -> Outcome: ...
    def reset(self) -> None: ...
```

## 意味論的契約

### `step(now, event)`

1. **単調時間**。`now` は `>= last_now` でなければならない（ここで
   `last_now` は前回の呼び出しで渡された `now`。初回呼び出し前は
   未設定）。`now < last_now - 1e-9` の場合、スケジュールは
   `ScheduleStateError` を送出する。
2. **イベントタイムスタンプ整合性**。`event` が `None` でない場合、
   `|event.time - now| <= 1e-9`。そうでなければ `ScheduleStateError`
   を送出する。
3. **呼び出しごとに単一 Outcome**。ちょうど 1 つの `Outcome` が返る。
   前回のステップから経過した時間が複数のスケジュール間隔より大き
   くても、本ステップで発火する強化子は最大 1 つ（時間ベース・
   スケジュールは発火後に `now` で再アンカーする）。
4. **`event=None` で冪等**。純粋な tick が強化子を生成するかどうかは
   スケジュール・ファミリに依存する:
   - 比率スケジュール (FR, VR, RR): tick では決して強化しない。
   - 間隔スケジュール (FI, VI, RI): tick では決して強化しない。
   - 時間ベース・スケジュール (FT, VT, RT): スケジュールされた時間が
     経過した後の tick で強化する可能性がある。
   - 分化強化スケジュール (DRO): resetting バリアントは、間隔が
     反応の介入なしに経過した tick で強化する。momentary バリアント
     は間隔境界で強化する。
   - 複合 (Concurrent, Multiple, Chained, Tandem, Alternative): 
     いずれかの活性コンポーネントが強化する場合に限り tick で強化
     する可能性がある。
5. **強化子タイムスタンプ**。発生した `Reinforcer` はすべて
   `Reinforcer.time == now` を持つ。
6. **最初のステップでアンカー**。「アンカーからの経過時間」を追跡
   するスケジュール (FT, VT, RT, DRO) は、受信した最初の `now` で
   内部クロックをアンカーする。したがって最初のステップでは決して
   強化しない。

### `reset()`

スケジュールを構築直後の状態に戻す。

- RNG 状態が復元される:
  - シードを所有するスケジュール (VR, VI, VT) は元のシードから再度
    シードする。
  - `rng: random.Random` を借用するスケジュール (RR, RI, RT) は構築
    時に RNG 状態をスナップショットし、復元する。
- カウンタはゼロに、シーケンス・カーソルは巻き戻り、シーケンス・
  プールはシード付きならビット同一に再生成される。
- `last_now` と時間アンカーは `None`（未設定）になる。
- 複合スケジュールは全コンポーネントに `reset()` を伝播し、続いて
  自身の記録をクリアする。

`reset()` は構成済みパラメータ（比率、平均、ステップ関数、コンポ
ーネントリスト、COD 値など）を **変更しない**。

## 値オブジェクト

4 つすべて不変。Rust では `#[derive(Clone, Debug, PartialEq)]` を
使用する。

### `ResponseEvent`

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct ResponseEvent {
    pub time: f64,
    pub operandum: String,  // default "main"
}
```

```python
@dataclass(frozen=True)
class ResponseEvent:
    time: float
    operandum: str = "main"
```

- `operandum` は押された operandum（レバー、キー、ボタン）を識別
  する。`Concurrent` がイベントをルーティングするために使用する。
- デフォルト `"main"` は非複合スケジュールで使用される。複合
  スケジュールがルーティングに使用しない限り、値は検査されない。

### `Observation`

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct Observation {
    pub time: f64,
    pub response_count: u64,  // default 0
}
```

```python
@dataclass(frozen=True)
class Observation:
    time: float
    response_count: int = 0
```

Observation は現在ランタイム自身では使用されていないスナップ
ショット（将来の analyser フック用に保持）。

### `Reinforcer`

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct Reinforcer {
    pub time: f64,
    pub magnitude: f64,   // default 1.0
    pub label: String,    // default "SR+"
}
```

```python
@dataclass(frozen=True)
class Reinforcer:
    time: float
    magnitude: float = 1.0
    label: str = "SR+"
```

- `magnitude` は量（ペレット数、ml、ポイント）をエンコードする。
- 負の magnitude + `label="SR-"` で嫌悪制御をモデル化する。
- 本ライブラリのすべてのスケジュールは、実験用ラッパが上書きしない
  限り magnitude `1.0`、`label="SR+"` で強化子を発生させる。両ポートで
  デフォルトは一致する。

### `Outcome`

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub reinforced: bool,
    pub reinforcer: Option<Reinforcer>,
    pub meta: BTreeMap<String, MetaValue>,
}
```

```python
@dataclass(frozen=True)
class Outcome:
    reinforced: bool = False
    reinforcer: Reinforcer | None = None
    meta: dict[str, object] = field(default_factory=dict)
```

**構築時に強制される不変条件**（両ポート）:
- `reinforced == true` ⟺ `reinforcer.is_some()`。

Rust は不変条件を強制する安全なコンストラクタのみを公開する:

```rust
impl Outcome {
    pub fn unreinforced() -> Self { ... }
    pub fn unreinforced_with_meta(meta: BTreeMap<String, MetaValue>) -> Self { ... }
    pub fn reinforced(r: Reinforcer) -> Self { ... }
    pub fn reinforced_with_meta(r: Reinforcer, meta: BTreeMap<String, MetaValue>) -> Self { ... }
}
```

`meta` は複合スケジュールがどのコンポーネントが発火したかを表面
に出すために使用される:

- `Concurrent`: 強化子が COD によりゲートアウトされたときに
  `cod_suppressed: bool` と `operandum: str` を設定する。
- `Multiple` / `Chained`: 現在活性なコンポーネントの刺激ラベルを
  `current_component: str` に設定し、`Chained` の非終端リンク遷移時
  には `chain_transition: bool` を設定する。
- `Tandem`: `current_component: int`（活性リンクインデックス）を
  設定する。
- `Alternative`: `alternative_winner: "first" | "second"` を設定する。

言語横断 fixture のため、`meta` の値は JSON プリミティブ (bool, int,
string) に制限される。両ポートでこれを遵守する。

## エラー分類体系

### Rust

```rust
#[derive(Debug, thiserror::Error)]
pub enum ContingencyError {
    #[error("schedule configuration error: {0}")]
    Config(String),

    #[error("schedule state error: {0}")]
    State(String),

    #[error("hardware error: {0}")]
    Hardware(String),
}

pub type Result<T> = std::result::Result<T, ContingencyError>;
```

`Schedule` トレイトの `step()` は `Result<Outcome>` を返す。Rust crate
ではすべての状態違反で panic ではなく `Err` を優先する。

### Python（正典）

```
ContingencyError (base)
├── ScheduleConfigError (also ValueError)
├── ScheduleStateError (also RuntimeError)
└── HardwareError
    └── NotConnectedError (also RuntimeError)
```

| Python クラス | Rust variant | 発生条件 |
|---|---|---|
| `ScheduleConfigError` | `ContingencyError::Config` | 不正なコンストラクタ引数（負の比率、未知のコンビネータ） |
| `ScheduleStateError` | `ContingencyError::State` | `step()` 契約違反（非単調時間、event/now の不一致） |
| `HardwareError` | `ContingencyError::Hardware` | HAL I/O、トランスポート、設定、依存の欠如 |
| `NotConnectedError` | `ContingencyError::Hardware`（プレフィックス付きメッセージ） | `connect()` 前または `disconnect()` 後の HAL read/write |

`NotConnectedError` は Rust では意図的に `Hardware(..)` に内包する。
両者を区別する必要がある呼び出し側はメッセージ文字列のプレフィックスを
検査する。`correspondence.md` の Gap G7 参照。

## 時間許容誤差

浮動小数点比較を統治する単一の定数。Rust では `src/constants.rs` の
モジュール定数として公開:

```rust
pub const TIME_TOL: f64 = 1e-9;
```

Python（正典）では `interfaces` モジュール内で `_TIME_TOL = 1e-9` として
定義される。

両ポートで同一に適用される:

- 単調時間チェック: `now < last_now - TIME_TOL` で失敗。
- イベント/now マッチング: `|event.time - now| > TIME_TOL` で失敗。
- 経過時間チェック: `now - anchor + TIME_TOL >= interval` で発火。
- スライディング・ウィンドウ eviction (DRH): `w[0] < cutoff - TIME_TOL`
  で eviction。
- LimitedHold 失効: `now > arm_time + hold + TIME_TOL` で失効。

この定数は conformance コーパスにとって load-bearing（負荷を支える）。

## スレッディング・モデル

Python 実装はスレッドセーフでは **ない**。呼び出し側はスケジュール
ごとにアクセスを直列化することが期待される。Rust crate は自明に
`Send` にできるが、明示的な同期なしでは `Sync` にしない — トレイトの
`&mut self` 規約により、借用チェッカが単一スレッドアクセスを強制する。
