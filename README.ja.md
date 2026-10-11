# Lookwright

> **スマートグラス・空間AIのための、ヘッドレス知覚 & 6DoFモーションテストフレームワーク。**

空間アプリのためのPlaywright。装着者の頭と目の動きをデータとして記述し、
「何が見えるべきか」をアサートします。**ヘッドセットも、GPUも、ディスプレイも不要。**
ごく普通のLinux CIマシン上ですべてが動きます。

| | |
|---|---|
| ステータス | v0.1.0 — 全フォーマットに `"version": 1`（[SPEC.md](SPEC.md) が唯一の正規仕様） |
| ドキュメント | [daikiito-dk.github.io/Lookwright](https://daikiito-dk.github.io/Lookwright/) |
| 言語 | Rustワークスペース + TypeScript SDK |
| レンダリング | 決定論的CPUラスタライザ（Vulkan/WebGPU/Mesaは一切不使用） |
| ライセンス | MIT |

## なぜ必要か

スマートグラスアプリ（Ray-Ban Meta、Orion、Horizon OSデバイス）のテストは、
普通は15万円超のヘッドセットを被って歩き回ることを意味します。これはCIに載りません。
Lookwrightは頭の動きと視線をデータファイルからリプレイし、シーンと空間UIを
CPU上でレンダリングして、「装着者が歩き回って見回してもUIはアンカーされ続けるか？」を
赤/緑の終了コードを持つ再現可能なテストに変えます。同じシミュレーションが構造化された
**知覚フィード**（可視オブジェクト、バウンディングボックス、注視ターゲット）も出力するので、
そのままコンテキスチュアルAIエージェント（例: Llama vision）に流し込み、
その判断をユニットテストできます。

```
┌────────────────────────────────────────────────────────┐
│             CLI / GitHub Actions Runner                │
│        lookwright test · golden · perception           │
└───────────────────────────┬────────────────────────────┘
                            │
┌───────────────────────────▼────────────────────────────┐
│                    Lookwright Core                     │
│  - 軌道エンジン（6DoF 頭部/視線モーション）              │
│  - UIアンカーエンジン（ワールドロック/ヘッドロック）      │
│  - 決定論的CPUレンダラ（GPU不要）                        │
│  - 空間知覚モック（BBox、Depth、注視レイ）               │
└────────────┬───────────────────────────┬───────────────┘
             │                           │
┌────────────▼─────────────┐ ┌────────────▼──────────────┐
│   テスト対象の空間UI      │ │  テスト対象のコンテキスト   │
│   （アンカー/レイアウト） │ │  AI（視覚&注視プロンプト）  │
└──────────────────────────┘ └───────────────────────────┘
```

## クイックスタート

```bash
git clone https://github.com/daikiito-dk/Lookwright && cd Lookwright

# 同梱のサンプルをエンドツーエンドで実行（アサーション + ゴールデン画像）
cargo run -p lookwright-cli -- test --config examples/tests/walk_and_look.test.json

# 合成ワールドを確認
cargo run -p lookwright-cli -- worlds list
cargo run -p lookwright-cli -- worlds show living_room

# AI知覚フィードをダンプ（Llama系エージェントへのモック入力）
cargo run -p lookwright-cli -- perception \
  --world living_room \
  --trajectory examples/trajectories/walk_and_look_left.json \
  --at-sec 1.0 --format jsonld
```

パスした実行の典型的な出力:

```
lookwright: walk-and-look
A-01  PASS  ui_visible info-card: visible 5.53s of 8.00s (required 2.00s, coverage 0.99)
A-02  PASS  gaze_focused tv: gaze on tv for 4.43s (required 1.00s)
A-03  PASS  anchor_drift info-card: max drift 0.0000m within 0.0100m
A-03  PASS  anchor_drift hud-hint: max drift 0.0000m within 0.0100m
A-04  PASS  event_fired pinch: event gesture fired at t=5.000s
A-04  PASS  event_fired voice: event voice fired at t=6.800s
A-05  PASS  golden_visual: 2 frame(s) within thresholds (worst mean_abs_diff 0.000, changed_pixel_ratio 0.00000)
summary: 7 pass, 0 fail
```

終了コード（SPEC.md §10）: `0` パス、`1` アサーション失敗、`2` 用法/バリデーションエラー。
だからLookwrightはどんなCIにもそのまま載ります。

## 3つのファイルタイプ

1. **軌道（trajectory）**（`examples/trajectories/walk_and_look_left.json`）— 6DoF頭部
   キーフレーム（`t_sec`、`position_m`、回転はクォータニオンまたはオイラー）、
   頭部ローカルの注視角度、注入イベント（`gaze_at`、`gesture`、`voice`）。
   JSONまたはYAML。
2. **ワールド** — ビルトインプリセット（`living_room`、`office`、`urban_street`、
   `warehouse`）、ラベル付きボックス/点群のJSONワールド、または任意のglTF 2.0
   シーン（`.gltf`/`.glb`）。
3. **テスト設定**（`examples/tests/walk_and_look.test.json`）— ワールド +
   軌道 + カメラ + UIパネル + アサーション。

```json
{
  "world": "living_room",
  "trajectory": "../trajectories/walk_and_look_left.json",
  "ui": [
    { "id": "info-card", "size_m": [0.40, 0.25], "color": "#00ccff",
      "anchor": { "mode": "world", "position_m": [0.0, 1.9, -3.4] } }
  ],
  "assertions": [
    { "type": "ui_visible", "ui": "info-card", "duration_sec": 2.0 },
    { "type": "gaze_focused", "target": "tv", "duration_sec": 1.0 },
    { "type": "anchor_drift", "ui": "info-card", "max_drift_m": 0.01 },
    { "type": "golden_visual", "dir": "../golden/walk_and_look",
      "times_sec": [1.0, 5.0] }
  ]
}
```

## アサーション（SPEC.md §9）

| コード | `type` | パス条件 |
|--------|--------|----------|
| A-01 | `ui_visible` | パネルが `duration_sec` だけ可視（カバレッジ ≥ `min_coverage`） |
| A-02 | `gaze_focused` | 注視レイが `target` に `duration_sec` ヒット |
| A-03 | `anchor_drift` | ワールド/ヘッドロックの配置ドリフトが `max_drift_m` 未満 |
| A-04 | `event_fired` | ジェスチャー/音声/注視イベントが実行中に発火 |
| A-05 | `golden_visual` | レンダリング済みフレームがゴールデンPNGと閾値内で一致 |

`wobble` フォールト注入（SPEC.md §5）で、A-03が悪いアンカリングを確実に
検出することを証明できます。意図的に失敗する
`examples/tests/faulty_anchor.test.json` を参照（`lookwright test` は終了コード1）。

## GPUなしのビジュアルリグレッション

`lookwright golden update --config <cfg>` でゴールデンフレーム
（`frame_<t_millis>.png`）を再レンダリング。`test` は同じフレームを再レンダリングして
`mean_abs_diff` / `changed_pixel_ratio` 閾値で比較するため、プラットフォーム間の
libm差（macOS ARM vs Linux x86）でCIがフレークしません。
ゴールデンは生成するものであり、手編集禁止です。

## コンテキスチュアルAIのための知覚フィード

```bash
lookwright perception --world living_room --trajectory walk.json \
  --at-sec 1.0 --format jsonld
```

```json
{
  "@type": "PerceptionFrame",
  "schema": "lookwright/perception-frame@1",
  "t_sec": 1.0,
  "camera": { "position_m": [0.0, 1.74, -0.6], "forward": [0.0, -0.0436, -0.9990],
              "gaze_target": "tv" },
  "visible_objects": [
    { "@type": "SpatialObject", "id": "tv", "label": "TV", "distance_m": 4.32,
      "bbox_px": [266.30, 186.86, 373.70, 246.78],
      "world_aabb_m": { "min_m": [-0.8, 0.85, -4.94], "max_m": [0.8, 1.75, -4.86] } }
  ]
}
```

これをエージェントのプロンプトパイプラインに流し込み、その出力をアサート。
「装着者は何を見ているか？」のロジックがユニットテストになります。

## TypeScript SDK / npx

```ts
import { runTest } from "lookwright";

const { exitCode, report } = runTest("spatial/walk_and_look.test.json", {
  reportPath: "report.json",
});
```

```bash
npx lookwright test --config spatial/walk_and_look.test.json --junit junit.xml
```

ラッパーはネイティブバイナリを `$LOOKWRIGHT_BIN`、次に `PATH` 上の
`lookwright` から解決します（`sdk/ts/README.md` 参照）。

## CI

- GitHub Actions: `.github/workflows/ci.yml` が fmt + clippy + 全テスト +
  同梱サンプルのセルフテストを実行し、JUnitレポートを公開します。
- 他リポジトリ向け: `examples/ci-github-action/` に再利用可能な
  コンポジットアクションがあります。
- Docker: `docker build -t lookwright .`（CPUのみのイメージ、
  ランタイムは `debian:bookworm-slim`）。

## レイアウト

```
crates/lookwright-core        6DoF軌道、カメラ数学、UIアンカー
crates/lookwright-perception  ワールド（プリセット、JSON、glTF）、レイキャスト、フラスタム、フィード
crates/lookwright-renderer    決定論的CPUラスタライザ、PNG、画像差分
crates/lookwright-cli         `lookwright` バイナリ
sdk/ts                        TypeScript SDK + npxラッパー
examples/                     軌道、テスト設定、ゴールデン、CIアクション
SPEC.md                       唯一の正規仕様（フォーマット + コード体系）
AGENTS.md                     このリポジトリで作業するAIコーディングエージェント向けルール
```

## 開発

```bash
cargo test --workspace
cargo fmt --all && cargo clippy --all-targets -- -D warnings
(cd sdk/ts && npm ci && npm test)
```

`SPEC.md` が規範です: コードと仕様が食い違ったらコードを直します。
バリデーションコード（`T-*`、`W-*`、`C-*`、`P-01`）とアサーションコード
（`A-01`…`A-05`）は公開APIです。完全な作業規約は `AGENTS.md` を参照してください。

## ロードマップ（SPEC.md §16）

GPUアクセラレーション付きオフスクリーンレンダリング（ヘッドレスVulkan/WebGPU）、
OpenUSDとPLY/LAS点群インポート、Horizon OSオンデバイスハーネス、
サブサンプル単位のアサーションタイミング、ピクセル単位のオクルージョンマスク。
