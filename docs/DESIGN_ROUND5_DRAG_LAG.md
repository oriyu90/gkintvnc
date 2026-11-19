# GlintVNC ラウンド5設計案 — リモート・ウィンドウのドラッグ破綻と遅延の徹底改善 (2026-11-19)

ユーザー報告 (2026-11-18 夜 / システム日付 2026-11-19):

> リモートデスクトップ先のウィンドウを移動させる時とか、windowのフレームをクリックで長押ししながらカーソルを移動させてるのに、下のボタンに判定が行って動かせなかったり、動きがかくかく(接続の画面がラグい)

## 0. 現状コードから特定した事実（ソース実測）

### A. 浮動バーがキャンバスの上に重なり、クリックが二重配送されている
- セッション窓の浮動バーは `Area(Order::Foreground)` で **fb キャンバスの上**（top-center, y=10）に描かれる（`ui.rs` の `BAR_HOVER_PT=42.0` 近傍）。
- その領域は remote 画像の**上**にあるため、`remote_input` はその位置を `map_point` できる → **fb の最上部 row に press が送られつつ**、egui 側ではバーのボタンも press を受ける。つまりバー上のクリックが **fb のタイトルバー行とバーのボタンの両方**に届く。macOS 側で「切断」や「設定」を押すつもりのクリックがリモート側でタイトルバー press になり、ドラッグ開始と誤認 → 画面がフルリフレッシュストーム（かくかく）。
- バーは `button_mask != 0`（ドラッグ中）でも表示され続ける → リモートのタイトルバー/フレーム（フルスクリーンでは画面最上部=ちょうどバーの下）を **バーが覆って操作を奪う**。

### B. フレーム更新がフルバッファで走っている
- サーバが 1920×1080 Raw/CopyRect を送るたびに、プロトコルスレッドが `rgba.clone()`（**約8MB/回**）を `Event::Frame` に詰め、`sync_channel(2)`（容量2）にブロック送信。受け側は 8MB を `ColorImage::from_rgba_unmultiplied`（さらに約8MBコピー）で複製し、`tex.set` を **イベントごとに**呼ぶ（1ドレインで複数回のフルテクスチャ転送）。
- `sync_channel(2)` が満杯だとプロトコルスレッドが `send` で**ブロック**する → TCP バッファが塞がり、サーバ側でフレームが滞留 → 表示は常に古いフレームを追う（伝統的な VNC ビューアの「遅れが溜まる」問題）。
- セッション窓は `request_repaint_after(16ms)` を無条件に呼んでおり、**アイドル中も 60fps で全面再描画**（Intel GPU に恒常負荷、かつバーの時間駆動非表示のためだけ）。

### C. 帯域
- Raw のみ → フルフレーム 8.3MB。小さな dirty rect が複数来ても、B の8MB経路を通る。サーバが ZRLE(16) を送れる構成なら ~10-30% に圧縮可能。

### D. 検証済みの正しい部分（維持する）
- PointerButton 離散イベント駆動のボタンマスク・fb端clamp streaming・リリース exactly-once、カーソル疑似符号、LED State、F1–F35、Text 経路、スクロール積算は現行どおり。`map_point` の逆変換は数学的に正しい（scale誤差なし）。

## 1. 設計（5つの修正）

### 修正1: 浮動バーの入力排他と非表示条件
- `SessionShared` に `bar_rect: Option<egui::Rect>` を追加し、バーを描いたフレームに外側 AABB を保存。
- `remote_input` は **press / release / motion を、bar が表示されていてポインタが bar_rect 内なら fb へ送らない**（バーはローカル専用）。ドラッグ中（mask!=0）はバー自体を描画しない（下記）のでこの条項は発動しない → ドラッグは常に fb 到達。
- **`button_mask != 0` の間はバーを描画しない・hover-reveal も再アームしない**。リリース後にポインタがまだ top なら通常どおりバーを出す。
- これにより「バーのボタンを押したのに fb が反応」「タイトルバー行のクリックがバーに吸われる」の両方が解消。

### 修正2: フレーム更新をダーティ矩形ストリームへ（背圧解消）
- `protocol::Event::Frame { width, height, rects: Vec<DirtyRect> }` へ変更（`rgba` フィールド削除）。
- `DirtyRect { x: u16, y: u16, width: u16, height: u16, rgba: Vec<u8> }` — サーバが更新した矩形のピクセルだけを転送。1080p 初期描画は全画面1矩形で従来と同等、差分更新は矩形サイズだけ。
- CopyRect/ZRLE/Raw すべて read_update 内で適用し、適用済み矩形を `Vec<DirtyRect>` に集めて送る。
- セッション窓は `SessionShared.image: egui::ColorImage`（永続）へ矩形を直接パッチし、`tex.set` を **1 UI フレーム 1 回まで**（ドレイン内で複数 Frame イベントが来ても `frame_dirty` フラグ1つで集約）。
- `sync_channel(2)` は維持（メッセージが小さくなるのでほぼ詰まらない。詰まっても「ベスト2枚最新」ではなく安全にブロック解除される設計上不要にしない）。
- `request_repaint_after(16ms)` の無条件タイマーを廃止 → バーが visible かつ `controls_visible_until` までの間だけ `repaint_after(差分)`。その他はイベント駆動（wake → 現行）のみ。

### 修正3: ZRLE (encoding 16) 実装

調査知見 (2026-11-19, ローカルの rfbproto.rst + web 調査票 — `docs/DESIGN_ROUND5_DRAG_LAG.md` 巻末参照):
- **TigerVNC/Xtigervnc サーバは SetEncodings の先頭から先勝ちでエンコーディングを決定**する。よって ZRLE を先頭に置いた (未指定時 Raw が選ばれ続ける)。
- **xfwm4 コンポジタ下では Xvnc が CopyRect を決して送出しない** (XRender 経路は全て "changed" 扱い、redirected window では winPixmap≠screen pixmap → is_visible()==false → CopyRect 不能; tigervnc #908)。つまりドラッグがカクカクする根本の1つは**サーバ側の描き直し量**で、クライアント側で圧縮する ZRLE が直接効く。
  - サーバ側の推奨事項(ユーザー確認済みでなければ実施しない): `xfconf-query -c xfwm4 -p /general/use_compositing -s false` でコンポジタOFF、または `vblank_mode=off`。
- **ZRLE 圧縮率**: 窓矩形+背景の再描画は ZRLE で数十分の一に圧縮できる (zlib level 2 が Xtigervnc の既定)。クライアント側 inflate は高速。
- Continuous Updates (-313 / message 150) は RTT を1フレーム分節約するが、(a) LAN RTT 0.5–2ms では実利が小さい、(b) UltraVNC 等の未対応サーバが message 150 を fatal として切断するリスクがある — **今回は見送り**。将来導入するなら rfbproto §ContinuousUpdates に従い、`-313` 広告 → 初回 update 後 `EnableContinuousUpdates(1, 0,0,w,h)` → 以後 incremental request を送らない / server message 150 (EndOfContinuousUpdates) を受理、をフラグ付きで実装すべき (既定OFF)。
- SetEncodings の並びは ZRLE(16) → CopyRect(1) → Raw(0) → DesktopSize(-223) → Cursor(-239) → LED(-261)。

- `SetEncodings` に ZRLE(16) を追加（Raw のまま維持、CopyRect 維持）。サーバが対応していればデルタ更新が zlib される（テキストUIで 5–10x 圧縮）
- 実装（`glintvnc-core/src/protocol.rs`）:
  - `U32 length` → zlib データを読む（`length ≤ rw*rh*4 + 64KB` を黙認上限として誤爆防止、超過は Protocol error）。
  - **接続ごとに永続 `flate2::Decompress`**（ZRLE は stream を接続横断で共有する規定）。64×64 タイル単位で左上から走査。
  - サブエンコーディング: 0=Raw, 1=solid, 2–16 packed palette, ≥128 の RLE 群（128 plain RLE, 130+ palette RLE）。CPIXEL は 3 bytes（我々の format: depth 24, true-colour, 24bit が LSB 3 bytes に収まる）。
  - 展開上限は `tiles × 64×64 × 4` を越える場合に即エラー。
  - RLE 長は `255` の連続 + 終端バイト（1 + 255×k + b）でデコード。
- `flate2`（pure Rust、デフォルト miniz_oxide）を core へ追加。unsafe 不使用。
- ピクセル配置は既存の Raw と同じ 32bpp LE BGRX → RGBA へ変換してキャンバスへ。
- 統合テスト（`tests/mock_rfb.rs`）: モックサーバが ZRLE 矩形を返す → ピクセル一致を検証（solid tile / packed palette / plain RLE / 非64倍数のタイル / 大きな実行長）。
- サーバが ZRLE を送らなくても従来経路で動く（フォールバック）。

### 修正4: ドラッグ追従性の最終調整
- `remote_input` の motion 送信は現行のマスク/clamp を維持。加えて **motion イベント自体の送信頻度は egui のフレーム依存**なので、drag 中は特に `request_repaint` が多いサーバ側で詰まらないことを修正2が担保。
- 修正2後も重い場合の次の一手（本設計には含めない）: Tight 符号化・連続更新(Continuous Updates -313/-312)、サブリージョン GPU アップロード（egui の安定APIには無い）。

### 修正5: 計測
- `GLINTVNC_DEBUG_FRAMES=1` で Frame イベントごとに `frame rects=<n> bytes=<n>` を eprintln（ピクセル値・内容は出さない）。
- LAN 実機で接続・再描画・ドラッグを体感試験。before/after の劣化点をこのログとキャプチャ(ウィンドウ単体のみ、即削除)で記録。

## 2. 実装順
1. 修正1（bar）→ 2. 修正2（dirty rect + coalesce + repaint gate）→ 3. 修正3（ZRLE + tests）→ 4. `cargo fmt/clippy`+`scripts/check.sh`+release build → 5. LAN 実機接続試験 → 6. DMG再生成・docs更新・SHA記録。

## 3. 残課題（正直に）
- egui 0.37 には `TextureHandle::set_partial(pos, image, options)` が存在し、矩形ごとの部分 GPU アップロードが可能（IronVNC と同じ方式）。本実装はこれを使い、GPU には常に変更矩形だけを送る。フルアップロードは connect/resize 時のみ。
- macOS の issue #5836（複数 viewport vsync 直列化）は未計測。修正2–4後に再評価する。
- 体感上のドラッグ追従はサーバ（XFCE/xfwm4 or Windows RealVNC）の損傷報告粒度にも依存する。クライアント側で潰せるのは描画パイプライン側のみ。


## 7. 実装結果・検証結果 (2026-11-19)

### 実装したもの
1. **フローティングバーの二重配送を根治**: `SessionShared.bar_rect` (バーの外枠、フレーム全域) を毎フレーム記録し、press/release/hover-motion が bar_rect 内かつ未ドラッグ (`button_mask==0`) の場合は fb に送らない。さらに **ドラッグ中 (`mask != 0`) はバーを一切描画せず、hover 再アームもしない**。
2. **ダーティ矩形ストリーム + 部分テクスチャアップロード**: `Event::Frame { width, height, rects }` に変更し、永続 CPU ColorImage (`SessionShared.image`) を直接パッチ、GPU には `TextureHandle::set_partial` で矩形だけを送る (初回/リサイズ時のみ全量、1080p で 8.3MB/1回)。無条件の 16ms セッション再描画を廃止 (バーが visible で自動隠しタイマーが pending の時だけ `request_repaint_after`)。
3. **ZRLE (16)**: 接続単位の単一 zlib ストリーム (`ZrleState` を `InflateUpkeep` として常駐)、64×64 タイル、サブエンコード 0/1/(2..16)/(128,130..255)、CPIXEL=3B (ネゴシエート形式が depth 24, RGB が LSB 3 バイトに収まるため)、RLE 長は `sum(bytes)+1`、パレットはタイル毎にリセット。`flate2` (rust_backend/miniz_oxide、pure Rust)。SetEncodings は **ZRLE→CopyRect→Raw→DesktopSize→Cursor→LED** の順 (TigerVNC は先勝ちで採用)。

### 検証結果 (正直に)
| 項目 | 結果 |
|---|---|
| `scripts/check.sh` (fmt/check/clippy/test) | PASS (40 tests: 28 core unit 含む ZRLE デコード群 + 2 i18n + 10 integration 含む ZRLE end-to-end) |
| モックループ接続 (`tests/mock_rfb.py --log-pointer`) | PASS (接続・フレーム適用 OK) |
| ZRLE デコード | PASS (単体 9 ケース + 統合 `zrle_rectangle_is_decoded_end_to_end`) |
| LAN 実機 (`192.0.2.10:5900`) | **PASS (2026-11-19 再テスト)** — TCP 778ms、`probe` silent、フルスクリーンセッションで実機のリモート XFCE デスクトップ（Thunar/端末/タスクマネージャ）を正確に描画。**dirty-rect streaming を実機で観測**: 初回 8.3MB の全面フレームの後、3-13 rect・約50-97KB/update（Round 4 までは毎 update 8.3MB + フルアップロード）。ウィンドウ単体キャプチャで確認後・即削除 |
| CGEvent による合成ドラッグ注入 | **INCONCLUSIVE** — 全画面セッションが macOS の Space 切替で on-screen リストに現れず、合成イベントがどのウィンドウにも届かなかった (環境依存のフォーカス不能)。物理マウスでの操作感確認が必要 |
| DMG | PASS (`ce0150a529cac0effbb8ea264119475af99ff430896dd53e54b5b1db28f09be7`, `--version` 0.1.0) |

### 未解決 / 残課題
- **ローカル・デグラデーション**: このマシンでは全画面セッションの Space 切替が不安定 (on-screen リストから消え、イベントが届かないことがある)。再発時はセッションを閉じて再接続すること。
- **GUI 自動化の限界**: この環境では CGEvent 注入が信頼できない（全画面セッションが Space 切替で on-screen リストから消え、合成イベントが届かないことがある）ため、ドラッグの体感・バー挙動・タイトルバーグラブ可否は実機 (物理マウス) での確認が必須。なおこの日の再接続テストでは全画面セッションは正常に表示・描画された。
- **リモートの xfwm4 コンポジタが根本原因の1つ**: サーバ側で CopyRect が出ないため、窓ドラッグ時の変更範囲が丸ごと再エンコードされる。`xfconf-query -c xfwm4 -p /general/use_compositing -s false` でコンポジタを切れば CopyRect が復活し、帯域は劇的に改善する見込み (tigervnc #908)。ユーザー判断で実施。
- Continuous Updates (-313) は互換リスク (未対応サーバの切断) と実利の小ささから見送り。導入時は既定OFF+オプトイン。
- 初回フレーム / リサイズ時のフルアップロード (8.3MB) だけが残る。これは egui の設計上避けられない。
