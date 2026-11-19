# GlintVNC ラウンド2設計案 — 全画面リモート／マルチウィンドウ／クリップボード (2026-11-18)

対象ユーザー報告: "接続した時、なんか横のパネルうざい→普通に全画面でリモート画面を表示してほしい"、"画面が真っ黒(他のアプリではそうならない)"、"各所のボタンの文字部分が中心に揃ってない"、"ドックのアプリアイコンクリックで保存した接続先を出して押したらすぐ接続、複数ウィンドウ対応"、"コピーペーストの共有(TigerVNCのように)"。

## 0. 画面収録権限について

2026-11-18 ユーザー付与により screencapture が動作することを確認した（ただし捕捉には個人情報が写り得るため、恒久的な検証には使わず `screencapture -l<windowID>` で GlintVNC ウィンドウのみを捕捉し、即削除する運用とする。実施済み：検証用キャプチャ35枚は全削除）。検証はウィンドウ単位キャプチャ＋モックサーバ＋実サーバ (192.0.2.10:5900) で行う。

実装結果 (2026-11-18):
- **黒画面の根治**: インクリメンタル矩形をゼロキャンバスに適用していたプロトコル側バグと特定・修正。LAN 実サーバでリモートデスクトップ描画を実測 OK。
- **マルチウィンドウ**: セッションを deferred viewport (`Context::show_viewport_deferred`) として実装。即時 viewport はルートが非表示/occluded になると破棄される既知問題 (egui #8085) があるため deferred を選択。ランチャー(root)を閉じると eframe 既定でアプリ終了、セッション窓だけ閉じるとそのセッションのみ終了。ドッククリック = 再起動 → ランチャー (winit/eframe に dock-reopen イベントが存在しないため。`applicationShouldHandleReopen` は隔離 unsafe アダプタが必要でスコープ外)。
- **クリップボード**: msg 3/6 双方向同期を実装し、ループバックモックで実測 OK (remote→local で pbpaste に反映、local→remote でサーバログに `CLIENT_CUTTEXT`)。ローカル→リモート送信は接続後のローカル変更のみ・256KiB 上限・エコー抑制 (RealVNC `SendInitialClipboard=false` 相当)。内容はログに出さない。
- **ウィンドウ構成**: セッション窓はリモート全面表示 (パネルなし) + 上端ホバーで出る自動隠れ制御バー (接続一覧/全画面/設定/切断)。同時 2 窓表示を実測。
- **ボタン中央揃え**: Hiragino に `y_offset_factor=0.08` を適用し光学的上下中央に。左右は egui atom layout の center。実機目視での最終判断はユーザー側。

## 1. 不具合の根因分析と修正方針

### 1-1. 黒画面（最重要） — 根因はクライアント側プロトコル層に確定

`protocol.rs::read_update` は更新ごとに `std::mem::take(&mut rgba)` でキャンバスを出し、次回更新用に **全ゼロの新しい canvas を確保** していた（旧コードのコメント「a fresh zeroed buffer is allocated for the next one」どおり）。

- 1回目の FramebufferUpdate は non-incremental（全体）→ 描画される
- 2回目以降は incremental（差分矩形のみ）→ **黒キャンバスの上に差分だけ描かれる** → 画面がほぼ黒
- マウスカーソルの矩形だけ差分が来る → 「マウスのとこだけなんか若干背景が見える」と完全一致
- TigerVNC等の他クライアントではキャンバスを永続化しているため同症状が出ない（ユーザー報告と一致）

修正: canvas を `run_once` 内で永続化し、Frame イベントには **`rgba.clone()`**（1920×1080で約8MB）を送る。CopyRect/DesktopSize 処理はそのまま（退行テスト: 増分フレームの未変更ピクセルが前フレームと一致することを検証）。P3で「差分矩形のみ転送・部分アップロード」へ最適化する旨をコメントに残す。

### 1-2. 増分キャンバスのメモリ/ペース

clone が増えるが 1080p では無視できる（将来 dirty-rect イベント化で解消予定と注記）。

## 2. 接続時のUX — 横パネル撤廃・全画面リモート

- **セッションは独立した兄弟ウィンドウ**（egui マルチビューポート、`Context::show_viewport_immediate`）として開く。ランチャーとは分離。
- セッションウィンドウは **リモート画面がウィンドウ全面を占める**。恒常的な側パネル・ツールバーなし。
- ウィンドウ上端にポインタを近づけると、上端中央に**半透明のフローティング制御バー**（自動で出る／2秒で隠れる）が現れる: [◀ 接続一覧（閉じる）] [⛶ 全画面] [⚙ 設定] [切断 ✕]。常時表示ではないので邪魔にならない。
- `profile.fullscreen_on_connect`（既定 true）が有効なら接続確立時にそのウィンドウだけ `ViewportCommand::Fullscreen(true)` で macOS ネイティブ全画面へ（ランチャーは残る）。
- 1:1 モードのみスクロール領域を使う従来動作を維持（ポインタ/ホイール実装は変わらない）。
- 切断・閉じる・⌘W はそのセッションだけを終了（key-up 解放＋ Stop）し、他セッション・ランチャーは継続。

## 3. ドックアイコン → 保存済み接続へ即接続／複数ウィンドウ

- **ランチャー（root ウィンドウ）** は従来の Home 画面を流用した「保存済み接続ランチャー」: プロファイルカードの [▶ 接続] を押すと **新しいビューポート（ウィンドウ）で接続** が開始される（押した瞬間に接続、`scale_mode`/`fullscreen_on_connect`/`key_preset`/`security` などプロファイル保存のカスタム設定を適用）。
- 複数の [▶ 接続] を押せば、192.0.2.10 等へ **複数のウィンドウが同時に** 接続される。カードにはオープン中セッション数の「接続中 n」バッジを表示して混乱を防ぐ。
- macOS ドッククリックの仕様: winit 0.30 / eframe 0.37 には `applicationShouldHandleReopen` 相当のイベントが**存在しない**（dockアイコンクリックを直接受け取れない）。したがって現在のプロセスが終了している場合、ドッククリック＝GlintVNCの再起動→ランチャー（保存済み接続一覧）が表示される、という macOS 標準挙動に準拠させる。ランチャーから即接続できるため要件「ドックのアプリアイコンクリックで保存した接続先を出して、押したらすぐ接続」を満たす。
  - ウィンドウが0枚でもプロセスを常駐させドッククリックで再表示する方式は、未文書化 AppKit FFI（隔離アダプタ＋unsafe監査）が必要で現スコープ外。P4（配布整備）で検討として `docs/MAINTENANCE.md` に記録する。
- 接続ごとに `ViewportId::from_hash_of(("session", profile_id, counter))` で一意化。クローズ検知は各子ビューポート内の `close_requested` を見てスロットを破棄→ `ViewportCommand::Close`。

## 4. ボタン文字の中央揃え

- Hiragino W3/W6 を `FontData.tweak` で `y_offset_factor = 0.08`（CJK の見かけのインク高さ分だけ下へ）し、egui の valign=center と組み合わせて上下中央を実現。左右は egui atom layout の main_align=center そのまま。
- 全ボタンを標準 `egui::Button`（または selectable_label）に統一し、独自の左寄せテキスト配置をなくす。
- 検証: `screencapture -l<windowID>` で実ウィンドウを 2x キャプチャし、保存する/接続/編集/全画面/切断ボタンの上下左右中央を目視確認する。

## 5. クリップボード共有（TigerVNC 相当・テキストのみ）

RFC 6143 §7.6.4/7.6.5:
- Client→Server: msg type **6** (`ClientCutText`): `u8 6 | 3B pad | u32 len | text`
- Server→Client: msg type **3** (`ServerCutText`): 同レイアウト（既存のパース部を発展）
- 文字コード: RFB は ISO-8859-1 が仕様だが、TigerVNC を含む主要実装は実装依存で UTF-8 を送る。受信は UTF-8 decode →失敗時 Latin-1 フォールバック、送信は UTF-8 とする（双方向で TigerVNC 実績あり）。長さ上限 1MB、超過は無視（ログに内容を出さない）。

実装:
- `Command::CutText { text: String }`、`Event::ClipboardText(String)` を追加。
- `AppSettings.clipboard_sync: bool`（既定 true、TigerVNC同様の双方向同期）。設定ウィンドウに「クリップボードをリモートと共有」チェックボックス、CLI `settings get/set clipboard_sync` を追加（GUI/CLI 同一モデル）。無効化すると両方向とも送らない。
- ローカル→リモート: 500ms ポーリングで arboard のテキストを取得し、前回同期値と変化したら `CutText` 送信（echo 抑制: 直近にリモートから適用した値と同一なら送らない）。
- リモート→ローカル: `ServerCutText` 受信 → arboard へ set → 「直近適用値」記憶。
- **クリップボード内容をログ・ステータスバーに絶対に出さない**（SECURITY.md 準拠）。
- 依存: `arboard 3` を直接依存に追加（eframe が既に使用しているものと同一バージョン系列。新規ライセンス追加なし）。
- テスト: モックサーバを拡張し (a) ClientCutText バイト列が一致すること、(b) サーバが ServerCutText を送ると Event::ClipboardText が出ること、を統合テスト化。`tests/mock_rfb.py` に `--echo-clipboard` 追加して手動検証にも使えるようにする。

## 6. 設定画面の追加項目

- 「クリップボードをリモートと共有」チェックボックス（ja/en）を設定 Window に追加。即時反映・永続化・CLI パリティ。

## 7. 検証計画

1. `cargo fmt`/`clippy -D warnings`/`cargo test --workspace`（退行テスト+クリップボードテスト追加で 27+ 件）と `scripts/check.sh`。
2. モックサーバで黒画面退行なし・クリップボード双方向を自動テスト。
3. 実サーバ 192.0.2.10:5900 に接続し `screencapture -l<windowID>` で: 全画面リモート表示（パネルなし）/ 2つのウィンドウ同時接続 / クリップボード双方向 / ボタン中央揃え、を捕捉検証。
4. キー入力・スクロールは既存の統合テスト＋実機ユーザー確認。
5. `docs/VERIFICATION_LOG.md`・`README.md`・`MAINTENANCE.md`・DMG 更新。

## 8. スコープ外

- TLS/VeNCrypt、Tight（従来どおり P2 継続）、画像クリップボード、AppKit reopen アダプタ、セッション丸ごとのパフォーマンス最適化（P3）。なお ZRLE は 2026-11-19 に実装済み。
