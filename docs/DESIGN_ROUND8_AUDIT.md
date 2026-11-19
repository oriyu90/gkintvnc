# GlintVNC ラウンド8 — 全ソース精査（安全性・安定性・GUI・設定・VNC互換）

実施日: 2026-11-19。対象: ワークスペース内の全 `.rs`（core / app / 統合テスト）、
`docs/`、`scripts/`、依存クレートの API（egui 0.37 / epaint / egui-winit）、
RFB 仕様（`rfbproto.rst`）および LibVNCServer / x11vnc の実装。
4 つの並列監査（メモリ・クラッシュ安全性 / 安定性・機能確実性 /
GUI・設定整合性 / VNC プロトコル互換）の結果を統合し、修正を実施した。

## 1. 監査で見つかった問題（重要度順）

### P0-1 悪意あるサーバーによるメモリ枯渇（CRITICAL, 既存コードで実証可能）
`read_update` は 1 更新あたり最大 16,384 矩形を受け付け、各矩形で
`extract_dirty_rect()` が `rw*rh*4` バイトを確保して `damage` に蓄積していた。
CopyRect は矩形ヘッダ 16 バイト + 座標 4 バイトだけで全画面コピーを要求できるため、
約 26 万バイトの通信で 4 TB の確保を誘発し、`handle_alloc_error` でプロセスが abort する。
ZRLE も小さな圧縮データから同じ増幅が可能だった。

**修正**: `MAX_DAMAGE_PER_UPDATE = 128 MiB` を導入。`push_damage_budgeted()` が
確保前に累積バイトを検査し、超過したら `Error::Protocol` で接続を終了する。
単体テスト `damage_budget_is_enforced_before_allocation` で「拒否された矩形が
バッファに残らない」ことを検証。あわせて `MAX_PIXELS` を 64 Mpx → 16 Mpx（64 MiB
キャンバス）に引き下げ、5K までは従来どおり、8K 以上は明示的に拒否するようにした。
これにより ZRLE の 64 MiB 出力上限（16 Mpx × 3 B = 48 MiB）とも整合した。

### P0-2 接続後の読み込み無期限ブロック（HIGH）
`TcpStream::connect` にだけ 8 秒タイムアウトがあり、greeting / 認証 / 矩形本体の
`read_exact` は無制限にブロックしていた。サーバーが矩形ヘッダだけ送って止めると
`Stop` コマンドは現在の `read_exact` を待つ間処理されず、ワーカースレッド・
ソケット・最大 64 MiB のフレームバッファが永久に残った（JoinHandle も未保持）。

**修正**: `run_once_with_timeout()` を追加し、greeting〜初期更新要求、各
`FramebufferUpdate` の読み取り、`ServerCutText`、メッセージ種別の読み取りを
`tokio::time::timeout(READ_TIMEOUT = 15s)` で包んだ。全 EOF（きれいな切断を含む）を
`Error::Io` として retryable に分類し、`auto_reconnect` が有効なら一貫して再接続する。
`SessionSlot` が `JoinHandle` を保持し、セッション削除時に Stop を送って
別スレッドで join する `reap_worker()` を追加（UI スレッドはブロックしない）。
テスト `stalled_server_hits_read_timeout`（300ms のタイムアウトを指定）を追加。

### P1-1 切断後も凍結フレームが残り再接続 UI が出ない（HIGH, 実証済み）
`Event::Closed` は `closed_message` を設定するだけで `s.frame` を残していたため、
`session_window_ui` の `if let Some(tex) = s.frame` が優先され、一度でもフレームを
受け取ったセッションでは「切断されました / 再接続 / 閉じる」オーバーレイが
表示されなかった。`auto_reconnect=false` のときは実質的に操作不能だった。

**修正**: `Event::Closed` で `release_held()` を呼び、`frame` / `image` を破棄する。
ボタン・修飾キーの解放も切断時に確実に送信する（AGENTS.md の必須要件）。

### P1-2 スクロールがドラッグ中のボタンを奪う（MEDIUM, 実証済み）
`send_scroll_pulse()` は `buttons = 8/16/32/64` と `0` を送っていた。RFB の
PointerEvent は現在のボタンマスク全体を送る規約なので、左ボタンを押しながら
ホイールを回すとドラッグ中に 2 回のボタン解放が発生していた。

**修正**: `send_scroll_pulse()` が `held = button_mask` を保持して
`held | wheel_bit` → `held` を送り、`send_pointer()` を経由して
スロットル / `last_pointer` も同期する。残ったホイール蓄積は 16ms 後の
repaint で継続処理する（`MAX_SCROLL_PULSES_PER_FRAME` で詰めても消失しない）。

### P1-3 Keychain が UI スレッドをブロック（MEDIUM, 規則違反）
`open_connection` / `respawn_session` / `save_draft` / `remove_draft` が同期的に
`keyring` を呼んでいた。Keychain がロックされているか初回の「常に許可」を
求められると、アプリ全体がフリーズする（AGENTS.md 禁止事項）。

**修正**: 接続ワーカー内で `secrets::get_password()` を実行し、失敗は `Event::Closed`
で通知する。保存・削除も別スレッドで実行し、結果を `GlintApp::secret_rx` で受け取って
ステータスに反映する。UI スレッドは Keychain API を一切呼ばない。

### P1-4 プロトコル準拠 / 相互運用の問題
- **DesktopSize（仕様違反, MEDIUM）**: キャンバスを破棄してゼロ埋めした後に
  `incremental=1` を送っていた。RFC 6143 は「フレームバッファを保持するか、
  次の要求の `incremental` を 0 にする」ことを要求する。→ `needs_full_refresh`
  フラグを導入し、次回要求を非増分に。テスト
  `desktop_size_forces_non_incremental_request` を追加。
- **カーソル hotspot（LOW/MEDIUM）**: RFB 矩形ヘッダは U16 だが TigerVNC / RealVNC は
  負の hotspot を S16（例 -1 = 0xffff）で送る。egui/winit は画像内の座標を要求するため
  カーソルが失信していた。→ `i16` として解釈して `0..=size-1` にクランプ。テスト
  `negative_cursor_hotspot_is_clamped` を追加。
- **LED State（LOW, 実機で無効）**: `-261` は QEMU 方式。主対象の x11vnc
  （LibVNCServer）は `0xFFFE0000` を使い、状態を矩形の x 座標に入れる（ペイロードなし）。
  → 両方を広告し両方を解析。テスト `libvncsrv_keyboard_led_state_is_parsed` を追加。
- **認証のサイレントダウングレード（LOW）**: パスワード保存済みなのにサーバーが
  None 認証だけを提示した場合、loopback で無警告に None に落ちていた。→ 明示エラーに。
  テスト `saved_password_never_silently_downgrades_to_none_auth` を追加。

### P2 GUI / 設定の不整合
- **設定項目の重複名（HIGH）**: `get_profile()` は id OR name で検索するため、
  同名プロファイルは別ホストへの接続を招きうる。`Store::validate()` で ID / 名前の
  重複を拒否し、GUI / CLI 共通の `save()` から呼ぶ。エラーは i18n キーに変換して
  日本語 UI でも英語生文字列を出さないようにした。
- **操作バーのキーボード経路が無い（HIGH）**: ホバー時のみ表示され、F8 / Esc の
  ローカル制御が無かった。→ F8 で表示切替、バー非表示時の Esc で全画面解除。
  F8 / ⌘R / Esc（全画面解除時）はリモートへ送らない。
- **`scale_mode` / `key_preset` が GUI から変更不能（MEDIUM）**: プロファイル編集フォームに
  3  segmented control を追加し、CLI に `--scale fit|pixel_perfect|stretch` を追加。
  未使用だった `viewer.scale_*` キーを再接続（re-wire）。
- **i18n の未使用キー 17 件（LOW）**: 削除し、両言語テーブルとの完全一致に加え
  「ui.rs のリテラルキーは ALL に存在する / ALL のキーは ui.rs で使われる」
  を検査するテスト `no_stale_or_unknown_ui_keys` を追加。
- **設定の矛盾（MEDIUM）**: `clipboard_sync` の既定値が `true` だったが、
  SECURITY / PRODUCT_SPEC は opt-in と明記 → 既定 `false` に変更。
  `default_fullscreen` は未使用の死に設定だった → 新規プロファイルの既定値として配線。
- **Enter / Tab の二重送信（LOW）**: egui が Key と Text の両方を出す環境で二重に
  なる問題を、同一フレーム内の Key イベント存在で抑制。
- **`ctx.request_repaint()` が間違った viewport を起こす（LOW/MEDIUM）**: ワーカーから
  呼ぶと直前の viewport（セッション窓）を起こし、ルートのイベント排水が最大 250ms
  遅れていた → `request_repaint_of(ViewportId::ROOT)` に変更。

### P3 ドキュメント / パッケージング
- `CFBundleVersion` が `1` のままだった → ビルド時に workspace version を設定。
- `README_JA.md` が「Raw のみ」「`.opencode/skills/` 8 件」など古い記述 → 全体的に更新。
- `SECURITY.md` の「key-up on disconnect は未実装」「トラックパッドスクロール未対応」は
  既に実装済み → 訂正。

## 2. 修正後の検証

| 項目 | 結果 |
|---|---|
| `scripts/check.sh`（fmt / check / test / clippy -D warnings） | PASS |
| テスト数 | 55（core 36 / i18n 3 / 統合 16）|
| 新規テスト | 12 件（damage budget / DesktopSize / LibVNCServer LED / カーソル hotspot / 認証降格 / タイムアウト / 重複名 / i18n スキャン ほか）|
| リリースビルド + DMG | PASS（`scripts/package-macos.sh`）|
| マウント確認 | `glintvnc 0.1.0`、`CFBundleVersion` = 0.1.0、ad-hoc 署名 `dev.glintvnc.viewer` |
| LAN 実機 probe | PASS（`Connected: test-server:0 (1920x1080)`、スクラッチ設定、画像は保存せず）|
| GUI 実機操作（ドラッグ / F8 / Esc / 再描画 / Keychain 再承認） | ユーザー確認待ち（Editor 側では GUI 操作を行わない）|
| DMG SHA256 | d5b475766bbad3aa72399c475e2039b654209a45d6ea000341537186e2489079 |

## 3. 意図的に先送りした項目（決定事項）

- TLS / VeNCrypt / ARD / RA2: 暗号化転送は ADR-0002 のとおり fail-closed。
- Tight / Hextile / Continuous Updates / Fence / ExtendedDesktopSize:
  実機 x11vnc では Raw/CopyRect/ZRLE で十分。追加すると解析面が広がるため見送り。
- `MAX_NAME=4096` / `MAX_CUTTEXT=1MiB` / 矩形数 16,384: 実測サーバでは十分に大きく、
  DoS 防御として維持（仕様上の上限より低いが、拒否は明示エラーでサイレント劣化はない）。
- entitlements / notarization: 現状 ad-hoc 署名のローカル配布のみ。
  Keychain の ACL を変える entitlment を推測で追加すると既存シークレットを
  破壊する恐れがあるため、Developer ID 取得時に別途レビューする。
