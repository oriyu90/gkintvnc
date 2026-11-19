> **注 (2026-11-19)**: 本ドキュメントはラウンド3時点の記録。以降、負債は返済済み: 「dirty-rect ストリーミングは P3 継続」→ **P3 ストリーミングはラウンド5 (2026-11-19) に実装済み・LAN 実機で実測確認済み**。「セッション窓の子は従来どおり 16ms 再描画」→ **無条件 16ms 再描画は撤廃済み**（バーの自動隠しタイマー分のみ）。詳細は `DESIGN_ROUND5_DRAG_LAG.md` を参照。

# GlintVNC ラウンド3設計案 — 遅延解消とリモートポインタ一致 (2026-11-18)

対象ユーザー報告 (2026-11-18, 実機検証後):
1. **ラグがひどい** — 他の VNC (TigerVNC 等) に比べマウスが飛び飛びで追従し、動作がワンテンポ遅れる。
2. **マウスポインタの座標がズレる** — Mac 側ポインタ位置と接続画面のポインタ位置を**ちょうど重なる**ようにしたい。Mac 側ポインタを画面端に動かすと接続画面の端で止まる（これは仕様）が、範囲内に戻ったら正確な位置へ滑らかに追従させたい。

## 0. 現状把握（コードから確定した事実）

### 0-1. 遅延の根本原因（特定済み・修正済み）
- 根因: `GlintApp::ui()` (root ビューポート) は `ctx.request_repaint_after(250ms)` でしか起床せず、`poll_sessions()`（ネットワークイベント drain → `TextureHandle::set`）も250ms周期だった。リモートからの Frame イベントは `sync_channel(2)` に滞留し、テクスチャ反映が最大 ~250–500ms 遅れる → リモート描画とリモートカーソルの追従がワンテンポ遅れる／飛び飛びに見える。
- 修正:
  - コア `run_once` に `Wake` (`Arc<dyn Fn() + Send + Sync>`) を追加し、**イベント送信成功ごとに `ctx.request_repaint()`** を呼ぶ（250ms の起床待ちが消える）。
  - `poll_sessions` で Frame 取り込み時に `ctx.request_repaint_of(vid)` を呼び、該当**セッションウィンドウだけ**を即時再描画。
  - drain 上限を 4 → 16 イベント/フレームに引き上げ。
- セッション窓の子は従来どおり 16ms 再描画（時間ベースの自動隠れバーに必要なため維持）。winit はポインタ/キーボード/ホイールイベント到達時にも repaint を要求するため、ポインタ追従は入力ドリブンでも動く（eframe の egui-winit `on_window_event` が `repaint: true` を返すことをソースで確認済み）。
- 画像の全面コピー（protocol 内 `rgba.clone()` + `ColorImage::from_rgba_unmultiplied`）は残るが ~8MB/frame @1080p で実測影響は小さい。dirty-rect ストリーミングは P3 継続。

### 0-2. ポインタの二重描画とエッジ挙動
- 現行: リモートサーバが fb 内に自前カーソルを描画済み（Raw エンコーディングのみ）。ローカル Mac のカーソルと**別々に**描画されるため、ネットワーク遅延ぶんズレて見えるのは必然。
- Mac 側カーソルを fb 範囲外へ出しても座標を送らない（サーバカーソルは最後の fb 位置＝端で停止）——これはユーザーが期待する挙動で、現行もそう動く。範囲内に戻れば即座に再送する（`last_pointer != Some(..)` による差分検知）。
- 追加修正（本ラウンド）:
  - **画像範囲外・ホバーなし・ウィンドウのフォーカス喪失・切断時に、押下中ボタンを必ず0へ戻す** (`release_buttons` / `release_held` に追加)。リモート側でボタンが押しっぱなしになる既知の問題を潰す。

## 1. 対応案（設計）

### 1-A. 遅延対策（実装済み §0-1）
→ 検証: 実サーバ (192.0.2.10:5900) に接続し、マウス移動・ウィンドウドラッグ・タイピングで追従性を体感確認。捕捉 (`screencapture -l<windowID>`) で描画内容一致を確認。

### 1-B. 座標のズレ対策 — RFB Cursor 擬似エンコーディングで「サーバカーソルを OS カーソルへ置き換える」
TigerVNC / RealVNC / noVNC が使う方式に合わせる。

- サーバが **Cursor 擬似エンコーディング (-239)** をサポートする場合、クライアントは SetEncodings に -239 を含めて要求 → サーバはカーソルを fb に描画せず、**カーソル形状（幅高・ホットスポット・ピクセル）を FramebufferUpdate の1矩形として送る**。以後カーソル形状が変わったときだけ送る。
- クライアントは受け取ったビットマップを egui の **`Context::set_cursor_image(Some(CustomCursorImage))`**（egui 0.37 / egui-winit で CustomCursorImage 対応をローカル registry で確認済み。winit の `create_custom_cursor` 経由で macOS の OS カーソル自体がサーバカーソル画像になる。同一形状は Arc identity で再アップロード抑制）に渡す → **Mac のカーソルとリモートのカーソルが物理的に同一の OS カーソルになるので、座標ズレは原理的に発生しない**。egui 側の hotspot は [u16;2] で、サーバのホットスポットをそのまま使う。
- ポインタが画像範囲外/ floating bar 上/ セッション終了時は `set_cursor_image(None)` で通常カーソルへ即時復帰。
- サーバが Cursor をサポートしない場合 (例: ローカルモック) は何も送られない → 従来どおり OS カーソル＋fb 内サーバ描画で動作（graceful degradation。プロトコル異常ではない）。
- 座標写像は現行の `fb = (pos - rect.min)/rect.size * fb.size`（`floor`）のまま変更不要。OS カーソルは常にマウス位置に描画されるため、fb 端で Mac カーソルの動きに制約はかからない（fb 端の外では座標を送らないのでサーバ側カーソルは端で停止＝ユーザーが許容した挙動）。
- リモートカーソルが fb 内に残った場合の保険として、hotspot 一致でカーソルが「ちょうど重なる」。

プロトコル層の変更:
- SetEncodings に `-239` を追加（既存 Raw/CopyRect/DesktopSize の後ろに追加、count=4）。
- `read_update` で enc == -239 の矩形をパース: `w,h`（カーソル形状）、ホットスポット、w*h*4 バイトのピクセル。ピクセルは fb と同じ 32bpp LE (B,G,R 並び) で送られてくる前提で、既存の Raw 変換と同じ BGRA→RGBA 変換を共通化して使う。w=0 または h=0 は「カーソルなし」を意味する。
- 新イベント `Event::Cursor { width, height, hot_x, hot_y, rgba }` を追加。canvas へ書き戻さない (fb には合成しない)。
- 統合テスト: モックが Cursor 矩形を送った場合に `Event::Cursor` が取り出せること、カーソル矩形が来ても canvas/Frame が破壊されないことをテスト。
- u8 幅として 1 rect on `w=u16,h=u16` を使う。hot の符号は RFC 準拠 (§要リサーチ確認)。

UI 層の変更 (session_window_ui):
- SessionShared に `cursor_icon: Option<egui::ColorImage>`-相当（`Arc<[u8]>` rgba + size + hotspot）を保持。
- root の poll_sessions で `Event::Cursor` を受けたら共有状態に保存し `request_repaint_of(vid)`。
- セッション窓描画時:
  ```
  let over_canvas = hover_pos が画像 rect 内;
  if over_canvas {
      if let Some(cur) = &s.cursor { ctx.set_cursor_image(Some(CustomCursorImage{ rgba: cur.rgba.clone(), size, hotspot })); }
  } else {
      ctx.set_cursor_image(None);
  }
  ```
  set_cursor_image は per-frame の出力に反映されるので、Pointer が範囲外に出た瞬間に通常カーソルへ戻る。
- ホットスポットはピクセル単位で `u16`。fb→スクリーン倍率が非整数でも OS カーソルはマウス位置に描画されるので視覚的ズレは 0（サーバカーソルの「fb 内位置」と fb→スクリーン写像が一致する限り）。

### 1-C. 「端で止まり、範囲内で滑らかに再追従」
- 画像 rect の外では PointerEvent を送らない（＝サーバカーソルは端で停止）。フォーカス喪失/範囲外でも押下ボタンは 0 にする（stick-button 防止）。再入時は差分検知で即座に送信 → 正確位置へ再追従。1-B の OS カーソル化後は、Mac 側カーソルは常に自由に動く（fb 端で止まるのはリモート側ポインタのみ）——TigerVNC/RealVNC と同じ挙動。

## 2. 実装手順
1. core: `Event::Cursor` 追加、`SetEncodings` に -239、Cursor 矩形パース、統合テスト（mock に `--send-cursor` 追加）。
2. app: Cursor イベント → OS カーソル化、画像範囲外/終了時 `set_cursor_image(None)`、押下ボタン解放修正、Wake 配線（済み）。
3. 検証: `cargo test` / clippy / fmt / `scripts/check.sh`、モック両対応テスト、実サーバで接続テスト＋捕捉（ウィンドウ単体、削除厳守）。
4. DMG 再パッケージ、VERIFICATION_LOG / MAINTENANCE / README 更新。

## 3. リサーチ確定版（2026-11-18 サブエージェント報告 + 一次ソース rfbproto.rst 直接確認）

- **-239 = Cursor**（"Rich Cursor", vendor `TGHT`/`RCHCURSR`）。IANA/RFC 6143 §7.7 準拠。矩形ヘッダの x/y は fb 座標ではなく**ホットスポット**（w/h はカーソル形状サイズ）。データ = `w*h*bytesPerPixel` の PIXEL（ネゴシエート済みフォーマットのまま、premultiply なし）＋ `floor((w+7)/8)*h` の 1bpp マスク（左→右、MSB先頭、走査線は整数バイトパディング）。**w=0 または h=0 → データなし＝カーソル非表示（デファクト標準）**。負のホットスポットは TigerVNC が S16 で送る実装差あり（0..32767 では U16 と同一）。
- 要求しても**未対応サーバは静かに無視される**（確認手段は「Cursor 擬似矩形を1回でも受信したこと」のみ）。fb 描画からカーソルを外すかは**サーバ実装依存**（TigerVNC/Xtigervnc は `needRenderedCursor()` で fb 描画にフォールバックする経路あり。X11 設定メニュー等でポインタがアプリ側に warp された場合など）。
- **-314 Cursor With Alpha**（32bpp RGBA、順序 R,G,B,A、各チャンネル premultiplied、先頭に S32 *encoding*）は「綺麗なカーソル」の最良コースだが、エンコーディングがサーバ選択（0=Raw を期待するが保証なし）で、premultiply 解除が必要 → 初版は -239 のみ実装（TigerVNC も Rich Cursor をサポート）。
- **egui 0.37 は OS レベルのビットマップカーソルをネイティブサポート**: `Context::set_cursor_image(Some(CustomCursorImage{ rgba: Arc<[u8]>（straight RGBA）, size:[u16;2], hotspot:[u16;2] }))`。eframe (wgpu/glow native) は `handle_platform_output_with_event_loop` 経由で自動適用（`winit::window::CustomCursor::from_rgba` → `event_loop.create_custom_cursor`）。Arc ポインタ identity でデデュープ（同一形状のフレーム再送でも再アップロードなし）。**確認: 2026-11-18 実機で `remote cursor 24x24 hotspot=(7,4)` を受信し、OS カーソルが X11 カーソル画像に差し替わったことを実測（Xtigervnc 1920x1080 環境）**。
- ローカルカーソル方式は TigerVNC (`Viewport::setCursor` → `Fl_RGB_Image` + `window()->cursor(...)`)、noVNC (`style.cursor='url(data:...) hotx hoty, default'`)、RealVNC (「Viewer ウィンドウではローカルシステムカーソルを隠せ」と公式明記) と同一パターン。**fb 内ダブルカーソルは -239 要求で解消される**。
- 座標の U16 値域: 0..65535。クライアントは `round→clamp(0, fb_size-1)` で送信する義務。逆変換を `as u16` キャストで行う負数 wrap は禁止。
- fb リサイズ直後は旧 fb 寸法で座標を送らないこと（現行実装も DesktopSize で canvas 再確保してから fb 寸法を更新しているので OK）。

## 4. 実装結果 (2026-11-18)

1. **遅延の根治**: コア `run_once` に `Wake` コールバック追加。イベント送信毎に `ctx.request_repaint()`。`poll_sessions` は Frame 取込毎に `ctx.request_repaint_of(vid)`。drain 上限 4→16。**懸念されていた 250ms 周期の描画滞留（ワンテンポ遅れ・飛び飛び追従の正体）を解消**。セッション窓の子は 16ms 再描画を維持（時間ベースの自動隠れバー用）。winit はポインタ/キー/wheel イベントでも repaint を要求する（egui-winit `on_window_event` で `repaint: true` を直接確認）のでポインタ追従は入力ドリブンでも動く。
2. **Cursor 擬似エンコーディング (-239) 実装**: SetEncodings に `-239` 追加。`Event::Cursor{width,height,hot_x,hot_y,rgba}` 新設。x/y を hotspot として読む（fb 座標ではない）。0×0 は「カーソル非表示」受理。統合テスト `cursor_pseudo_encoding_is_parsed` 追加（モックが 2x2 カーソルを送信 → パース内容を検証、Frame イベントも同時に来る）。`tests/mock_rfb.py --send-cursor` でも再現可能。
3. **OS カーソル差し替え**: セッション窓でポインタが画像 rect 内 → `set_cursor_image(Some(制御された画像))`、離脱/終了時 → `None`（通常カーソルへ即時復帰。floating bar 上でも通常カーソル）。サーバ未対応なら何も届かず従来挙動（OS カーソル＋fb 描画）に自然退化。
4. **押下ボタンの解放リークを修正**: 画像外/ホバーなし/フォーカス喪失/切断時に `PointerEvent buttons=0` を必ず送る（`release_buttons`）。リモート側でボタンが押しっぱなしになる問題を解消。
5. **診断フック**: `GLINTVNC_DEBUG_CURSOR=1` で受信カーソルのサイズ/hotspot（内容は出さない）を stderr に出力。
6. **実機検証**: 1920x1080 LAN TigerVNC に接続 → `remote cursor 24x24 hotspot=(7,4)` 受信・リモートデスクトップ描画 OK（先述、window-only 捕捉は個人情報保護のため即削除）。ローカルモックでも 2x2 カスタムカーソル受信を実測。
7. **残項目**: 実機での「飛び飛び解決」体感確認（fps 計測は不可）、Mac のフルスクリーン表示下でのポインタ精度、fb リサイズ跨ぎの座標。ユーザー実機確認を要する。
