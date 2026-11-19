# GlintVNC ラウンド6設計案 — リモートドラッグ操作の改善

対象問題（ユーザー再報告, 2026-11-19）:
「未だに接続先でドラッグ操作がやりにくすぎる」

本書は (1) コード監査で確定した client 側の欠陥 (2) 他アプリ実装例の調査結果 (3) 対応案の設計 (4) 検証計画 の4部で構成する。

## 1. Client 側監査結果（確定事項）

### 1.1 Pointer コマンドが silent drop され得る（重大）
- `remote_input` は全 Pointer/Key を `async_mpsc::Sender<Command> (capacity 128)` の **try_send** で送る。満杯時はエラーを捨てるだけで、**ボタン release（button_mask==0）自体が失われ得る**。
- worker が `read_update` のデコードで長時間占有される（§1.2）と 128 件は一瞬で埋まる。リリースが最後に捨てられると、リモートは left-down をラッチしたままになり、以降のクリックが「ドラッグの継続」に化ける → 「やりにくすぎる」の最有力の client 側原因。
- モーションは最新値での畳み込みなしに捨てられる（止まる→飛ぶ）。
- `release_held`（focus 喪失時の一括解放、`release_buttons` 含む）も同じ `try_send` で、満杯時は解放が届かない。

### 1.2 `read_update` 中はコマンドが完全に停滞する（重大）
- `tokio::select!` の一腕が `read_update(...)` 全体（プロトコル全矩形の読取＋デコード）。その間コマンド腕は1度も実行されない。
- サーバが大きな更新（xfwm4 コンポジット中の移動窓の全面再エンコード ≈ 数百KB〜数MB）を送ると、更新完了まで指の入力はサーバへ1件も届かない。

### 1.3 リモート再描画待ちの体感ラグ（構造的・client では除去不可）
- 「窓を掴んで動かす」はサーバ側が damage を返すたびに client が描画する。xfwm4 コンポジット時は XRender の変更が毎フレーム damage になり、CopyRect が使われず全再エンコード（TigerVNC #908; XRender 経路は CopyArea を使わない設計 `vncHooks.c`）。
- サーバの damage→送信は `-FrameRate`(既定60) とサーバ側輻輳制御に従う。

### 1.4 現行クライアント比較（他実装との差）
- **move の間引きが無い**: 生の hover/move イベントを毎回送る（TigerVNC/noVNC は 17ms・ボタン状態変化時は即送信）。
- 増分 FramebufferUpdateRequest は毎フレーム1本 = TigerVNC と同じ設計（問題なし）。
- Clock: rv は「増分要求は in-flight 1つ」明示管理（同等）。IronVNC は無管理。
- FrameReady 相当は sync_channel(2)＋ColorImage パッチでほぼ latest-wins 化済み。
- フォーカス喪失時にボタンも解放するのは TigerVNC 以上に安全（TigerVNC は FL_UNFOCUS で key のみ、pointer は release されない欠陥を持つ）。維持する。
- egui `ScrollArea` の drag-to-scroll は `DragScroll::OnTouch` が既定。マウス/トラックパッドでは発動しない。原因ではない（egui 0.37 source で確認）。
- macOS には OS レベルの pointer grab が無い（TigerVNC も macOS では取らない）。フルスクリーンでは画面端＝窓端なので clamp が効く。マルチモニタで他画面に出てから離した場合の release は egui に届かない可能性がある（既知の限界として文書化）。

## 2. 他アプリ実装例の調査結果

調査4件を実施（詳細は各セクション参照）。要点のみ抜粋。

### 2.1 noVNC (master, 2026-10-23)
- Mouse move を **17ms レート制限＋最新位置のみ保持**（`MOUSE_MOVE_DELAY=17`, rfb.js `_handleMouseMove`）。`this._mousePos` を上書きし、中間イベントは暗黙に捨てる（latest-wins）。ボタン press/release の前は `_flushMouseMoveTimer` で保留 move を吐いて順序を保証。
- PointerEvent には常にボタンマスクを同梱。mousedown で `setCapture` し window レベルで mousemove/mouseup を捕捉 → **キャンバス外リリースでもサーバへ正しい mask=0 を送る**。
- カーソル: RFB Cursor 疑似エンコード画像をローカルに描画（CSS cursor URI で遅延ゼロ）。サーバカーソルが完全透明のときだけ 3×3 ドットを出す `showDotCursor`（既定 false）。
- **Continuous Updates (-313) を常時広告し、サーバが返したら有効化**。品質・圧縮レベルの調整 UI あり。

### 2.2 TigerVNC (master, 1.16.80)
- ビューワは **17ms `PointerEventInterval`** で move を間引く（既定17ms、0で無制限）。**ボタンマスクが変化したときは即送信**。ホイールは press+release 直後送信。ローカルカーソルは常に Cursor 疑似エンコードを要求（OS カーソル差し替えは無し、FLTK ウィンドウカーソル描画）。
- フォーカス喪失時は keyboard の releaseAllKeys のみ（pointer は release されない＝残る欠陥）。
- サーバ側 (Xtigervnc): CopyWindow/CopyArea のみ CopyRect 化、**XRender (Composite 等) は全経路 `add_changed()` のみ → コンポジター有効時は構造的に CopyRect が出ない**。公式見解 (#908): 「DE が再描画している。MATE/XFCE + コンポジター無効で使え」。
- Continuous Updates (-313) は実装済み・自動有効化。**ただし fence (-312) をサポートしないクライアントには CU を拒否**。LAN では 1RTT 分しか縮まらず体感効果は軽微。効果は高遅延リンクで劇的。
- 設定に `use_compositing` や `PointerMotion` は存在しない。xfwm4 側の設定 (`xfconf-query -c xfwm4 -p /general/use_compositing -s false` または `xfwm4 --compositor=off`) で止めるのが唯一の実用的手段。
- ビューワの macOS 既知問題は keyboard/focus 系のみ。ドラッグ遅延の macOS 固有バグ報告は見当たらない。

### 2.3 RealVNC / UltraVNC / Apple / 動画系
- RealVNC: ローカルカーソル既定 (UseLocalCursor=TRUE)、`PointerEventInterval` パラメータ、QUALITY/ColorLevel で画質ノブ、CU (EnableContinuousUpdates) 実装、連続変化では request→response 往復を排除。
- UltraVNC: Local/Remote/Dot/No cursor モード。Mirror driver でダーティ矩形を直接受ける。Quick options に LAN 向け Ultra (LZO)。`OnlyPollOnEvent=1` は連続変化を間引くので不可。
- Apple Screen Sharing (Standard): Adaptive Quality（MVS codec 相当）。HP 接続は Apple silicon 同士のみ・Sonoma+ → 本機 (Intel/13.7.8) では不可。ARD 公式Tips: **無地デスクトップで差分量を減らすと速い**。
- 動画系 (CRD/AnyDesk/TeamViewer/Parsec): コーデックと輻輳制御で「体感の遅さ」を相殺。Parsec はホスト側合成カーソルでゼロ遅延カーソルは取らない。ローカルカーソル陣営は RDP/VNC + Moonlight。
- RFC 6143: ドラッグは「mask bit が立ったままの PointerEvent 列」。Long-press の概念は無い。Button mask は every PointerEvent に同梱。フォーカス喪失/切断時に held buttons を release するのは RFC 運用上必須（本プロジェクト方針と一致）。

### 2.4 IronVNC / rv (Rust)
- IronVNC: unbounded チャネル、~60Hz でそのまま send、カーソルは OS crosshair + painter の十字重ね。
- **rv** (方針が近い): tokio mpsc unbounded チャネル、**入力コマンド drain を Frame 先送りより先に行う**（key-up 遅延＝リモート側 autorepeat 防止が目的）、FrameReady を drain 側で latest-wins 畳み込み、MAX_EVENTS_PER_SLOT=64 の後 yield_now、増分要求 in-flight 1つ。
- moonlight: 絶対マウスは bounded queue で **latest-wins 上書き**（MOUSE_BATCHING_INTERVAL_MS=1）。
- **送信側 latest-wins スロットの実装例は両リポジトリに無い。採るなら新規設計（moonlight が実績）。**

## 3. 対応案

### A. Client 側入力パイプライン堅牢化（今回の本命・小規模）
1. **never-drop 経路**: UI→worker のコマンドチャネルを unbounded に変更（accept: 発生レートは throttle で ~60/s に制限されるのでメモリ増大は実質無し。IronVNC/rv と同じ）。motion は UI 側で 16ms 間引き（最新位置のみ保持、即座に捨てず保留→タイムアウトで flush）。**ボタンマスクが変化した場合は即送信**（TigerVNC 挙動）。press/release/key は常に enqueue。
2. **worker 側の矩形間 flush**: `read_update` に `cmd_rx` を渡し、各矩形の処理完了ごとに `try_recv` で溜まったコマンドを即 `send_command` する。ドラッグ中の巨大更新でも入力が矩形間で前進する（rv の入力優先 drain を RFB デコード境界に適用）。
3. pending move は button press/release の直前に flush（noVNC `_flushMouseMoveTimer` と同じ順序保証）。
4. フォーカス喪失時の release_held / release_buttons を never-drop 経路で送る。
5. **回帰テスト**: coalesce ロジック（16ms 間引き＋mask 変化即送信＋flush 順序）を純関数化してコアで単体テスト。`scripts/check.sh` green を確認。
6. （診断用）`GLINTVNC_DEBUG_FRAMES=1` の出力にエンコーディング別矩形カウント (raw/copyrect/zrle/desktopsize/cursor/led) を追加し、リモートがコンポジット ON/OFF かをクライアント実測で判定できるようにする。

### B. リモート側（ユーザー作業・効果最大・client では代替不可）
7. **xfwm4 コンポジター無効化**（xfwm4 側の設定）:
   `xfconf-query -c xfwm4 -p /general/use_compositing -s false`
   （または `xfwm4 --compositor=off`／設定→ウィンドウマネージャーの調整→コンポジター）
   これで CopyRect が復活し、ドラッグ時の再送量が激減する。**これが体感改善の最大のレバー。**
8. （任意・Apple ARD 公式TIP）デスクトップ背景を単色 (黒/グレー) にすると damage 面積が減り更に軽くなる。
9. （参考）無地でなくても "tight"/ZRLE で済むが、XRender 経路自体は消せない。

### C. 任意・後回し（今回は見送り推奨）
10. Continuous Updates (-313): LAN の RTT (<1ms) では 1フレーム分の1RTT しか縮まらず体感効果は軽微（TigerVNC 報告）。WAN 向けには有効だが、**fence (-312) 広告も必須**になるため実装コストが増える。**WAN 接続が必要になった時点で opt-in profile として検討。**
11. ExtendedMouseButtons (-316): マウスの Back/Forward ボタン拡張。現状対象外。
12. CaptureMethod 的な「grab の奪取」: macOS に OS pointer grab は無い。フルスクリーンの端まで行ったドラッグは egui に release が届かないケースがある（既知限界・文書化）。

### D. ドキュメント / 運用
13. macOS Force Click（トラックパッドでの長押しが副クリックや Look up に化ける）等の OS 側の挙動注意を README/MAINTENANCE に追記。
14. ドラッグ確認の手順書: コンポジター ON/OFF それぞれで `GLINTVNC_DEBUG_FRAMES=1` を見て CopyRect の出現有無を記録するフロー。

## 4. 検証計画
- `cargo fmt` / `cargo clippy` / `cargo test`（`scripts/check.sh`）
- 新規単体テスト: move coalesce 純関数群（A5）
- LAN 実機: probe PASS、実際の接続でウィンドウキャプチャ（確認後削除）
- 実効計測: A6 のエンコーディングヒストグラムでコンポジター無効化の効果を確認（CopyRect 出現）
- 実機ドラッグ感は物理マウスでの最終確認が必須（CGEvent 自動注入は再現性無し）

## 5. リスクと注意点
- unbounded チャネル化は「UI が無制限にメッセージを積める」ことを許すが、A1 の throttle で発生率を抑えるため実害はない（IronVNC/rv 実績）。
- worker 側 flush の実装で `read_update` のシグネチャを変える → コアのテストへの影響を最小化（フラグではなく新規引数、テストは従来とおり）。
- `Frame` の full-frame リクエスト挙動は変更しない（TigerVNC と同じ demand-driven を維持）。

## 6. 実装結果 (2026-11-19)

実装完了 (A1–A6)。テスト: `scripts/check.sh` PASS (44 tests: 32 core incl. MotionThrottle 4件 + 2 i18n + 10 integration)。
`GLINTVNC_DEBUG_FRAMES=1` にエンコーディング別矩形カウント追加 (bug修正: 初回は前置インクリメントになっており、その後分岐内へ移動)。

### 実機確認
- LAN (192.0.2.10:5900) プローブ PASS、ウィンドウモードでフルリモート描画を確認（窓キャプチャ確認後削除）。フレームログは `zrle=N` の増分のみ（初回 8.3MB → その後数KB〜100KB）。合成された PNG 上でリモート操作可能であることを確認したが、**ドラッグの体感はユーザーの物理マウスでの最終確認が必要**（CGEvent 自動注入は再現性無し、文書化済みの制約）。
- 全画面セッションは接続後に他 Space へ送られたため今回は窓モードで検証（設定はスクラッチのバックアップ経由で一時 off → 復元済み）。フルスクリーン Space の不安定さは既知のまま。
- **リモートの xfwm4 コンポジターをユーザー同意のもと SSH で無効化**: `xfconf-query -c xfwm4 -p /general/use_compositing -s false`、`xfconf-query` の読み戻し `false` を確認。パスワードは環境変数経由で1回のみ使用し、ファイル・argv・ログには残していない。ホスト鍵は StrictHostKeyChecking=accept-new で既知ホストに追加。

### 検証マトリクス
| 項目 | 結果 |
|---|---|
| cargo fmt / clippy -D warnings / test | PASS (44件) |
| LAN probe | PASS (silent) |
| LAN 実機・ウィンドウモード描画 | PASS (スクリーンキャプチャ確認後削除) |
| LAN 実機・全画面 | 未検証 (Space 不安定のため今回は見送り; 以前のラウンドで同構成の描画は確認済み) |
| コンポジター無効化 | PASS (読み戻し false) |
| ドラッグ体感 | PASS（2026-11-19: オーナー実機確認、本番完了）|
| DMG | SHA256 4d9f58412311e629259473784c2ce49a8af054bda5a0d788c5a5daf4030fe72b |

### 残る既知課題
- macOS に OS ポインタグラブは無い。マルチモニタ環境でウィンドウ外（別ディスプレイ）で button release した場合は取り逃がす（focus loss / disconnect で必ず解放する安全網はある）。
- Fullscreen Space 不安定（egui deferred viewport × macOS Space の相互作用と推定）。発生時はセッションを閉じて再接続。
- Continuous Updates (-313) は意図的に未実装（LAN では効果が薄く、fence -312 広告も必要; WAN 必要時に opt-in 検討）。
- ExtendedMouseButtons (-316) 未対応。
