# GlintVNC 保守ドキュメント (Maintainer's Guide)

対象読者: GlintVNC をビルド・配布・保守する開発者。ユーザー向けの使い方ではなく、**壊さないため・直すための実務手順**を記述します。
最終更新: 2026-11-19 / バージョン 0.1.0 / 対象: macOS 13 Ventura以降 Intel x86_64

---

## 1. リポジトリ構成

```
Cargo.toml ワークスペース定義 (version はここが真実の源)
crates/glintvnc-core VNC/RFBプロトコル・設定スキーマ・Keychain・再接続 (GUI非依存)
crates/glintvnc-app CLI + eframe (egui/wgpu) ネイティブGUI (バイナリ名: glintvnc)
assets/ Info.plist / GlintVNC.icns
scripts/ bootstrap / check / build / package / make-icon
tests/mock_rfb.py ループバック専用のRFBモックサーバ (Python)
docs/ 設計・セキュリティ・ロードマップ・検証ログ
```

## 2. ビルド環境の構築

1. macOS 13以降 (Intel) + Xcode Command Line Tools: `xcode-select --install`
2. Rust (rustup, stable): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` 
 `~/.cargo/bin` を PATH に追加
3. 初回セットアップ: `bash scripts/bootstrap-macos.sh` (環境診断、インストールは行わない)
4. ロックファイル再生成: `cargo generate-lockfile`、以後は `Cargo.lock` をコミットすること

## 3. 品質ゲート (変更をコミットする前に必ず)

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all
bash scripts/check.sh # fmt --check + cargo check + test + clippy -D warnings
```

`scripts/check.sh` が FAIL するコードは配布物に含めない。現在のテスト数は 21 (ユニット15 + 統合6)。

### 統合テストのモックRFBサーバ

`crates/glintvnc-core/tests/mock_rfb.rs` がプロセス内モック (None認証・Raw・CopyRect) を提供します。
別プロセスで動かす簡易モックは `tests/mock_rfb.py`:

```bash
python3 tests/mock_rfb.py --port 5999 --width 800 --height 450
# 別ターミナルで
export GLINTVNC_DATA_DIR=/tmp/glintvnc-test
glintvnc probe LocalMock # 事前に profiles add が必要
```

## 4. リリース手順

1. `cargo test --workspace` と `bash scripts/check.sh` がパスすることを確認
2. バージョンを上げる場合: `Cargo.toml` の `[workspace.package] version` を変更 →
 `cargo check` でロックファイル更新 → `assets/Info.plist` の `CFBundleShortVersionString` は
 パッケージスクリプトが自動同期する (LSMinimumSystemVersion や他キーは手動)
3. `bash scripts/package-macos.sh` を実行。以下が行われます:
 - `MACOSX_DEPLOYMENT_TARGET=13.0` でリリースビルド
 - `dist/GlintVNC.app` 組み立て (バイナリ + Info.plist [+ icns])
 - **ad-hoc 署名** (`codesign --sign -`)
 - `dist/GlintVNC-<version>-macOS-x86_64.dmg` 作成 (`/Applications` へのシンボリックリンク入り、UDZO圧縮)
4. 検証: `hdiutil attach dist/GlintVNC-*.dmg` → `glintvnc --version` / GUI起動 / `hdiutil detach`
5. SHA256 と手順を `docs/VERIFICATION_LOG.md` に記録 (現在の最新: `aa9f055c…` ad-hoc)

### 配布の既知の制約 (重要)

- 本リポジトリの署名は **ad-hoc のみ** です。配布先の Mac では Gatekeeper が初回起動をブロックします。
 回避方法を案内する場合は「DMGを開き GlintVNC.app を右クリック → 開く → 開く」と案内してください。
- ad-hoc 署名ではリビルドごとに cdhash が変わるため、Keychain の ACL が再プロンプトされることがあります。
 Developer ID による署名・公証を行わない限り、これは仕様上の制約です。
- 公証 (notarization) を行わない方針の場合、DMG に「Gatekeeper を右クリックで回避して起動」という説明書 (README) を同梱することを推奨します。

## 5. アンインストール / クリーンアップ

1. `GlintVNC.app` を削除
2. プロファイル削除前に Keychain エントリも消す:
 `GlintVNC.app/Contents/MacOS/glintvnc secret delete <プロファイル名>` (複数ある場合は各々)
 または Keychain Access.app で `dev.glintvnc.viewer.v1` サービスの項目を検索して削除
3. 設定ファイル: `~/Library/Application Support/GlintVNC/config.json` (+ `config.json.bak`) を削除
4. テスト用一時ファイル: `/tmp` や `GLINTVNC_DATA_DIR` 指定ディレクトリを削除

## 6. 設定ファイルとバックアップ 

- パス: `~/Library/Application Support/GlintVNC/config.json`
 (`GLINTVNC_DATA_DIR` 環境変数でテスト用に差し替え可能。全CLI/GUI操作でこれを使うと実データを汚さない)
- 形式: JSON (schema_version=1)。パスワード等の秘密情報は**絶対に含まれない** (`store` に secret フィールドなし)
- 保存は tmp ファイルへの書き出し → fsync → リネームの原子的置換。置換直前の旧版を `config.json.bak` にコピー
- 読み込み不能時は自動で `.bak` から復旧 (stderr に警告を出力して起動は継続)
- 手動復旧: `cp config.json.bak config.json`
- スキーマを変更する場合: `schema_version` を上げる、古い版からの移行コードを `load` に入れ、
 マイグレーションテストを `configuration.rs` の `tests` に追加すること

## 7. Keychain の扱い

- サービス名: `dev.glintvnc.viewer.v1` / アカウント: プロファイルのID (ランダム)
- CLI: `glintvnc secret set <selector> [--stdin]` / `secret delete <selector>`。`--stdin` は自動化用、対話時は非表示プロンプト
- GUI: プロファイル編集画面のパスワード欄 (空欄=変更なし)
- エラー時の確認: Keychain Access.app でサービス名検索 → 項目が出るか / ロック解除されているか
- **ログ・コミット・バックアップにパスワードを出さない**。トラブルシュート時も `config.json` ではなく Keychain の存在のみ確認する

## 8. ログと診断

- 現状ログ出力は stdout/stderr のみ (GUI を Finder から起動すると見えない)。調査時は:
 `/Applications/GlintVNC.app/Contents/MacOS/glintvnc` をターミナルから起動する
- API/クラッシュ前提: `Library/Logs/DiagnosticReports` (ユーザー領域) に .ips が出る。収集する場合はパスとスタックのみ共有し、デスクトップ名や接続先IPは不要なら伏せる
- 実機での再接続トレース: ステータスバー (ウィンドウ下部) のメッセージ遷移を記録する

### 自己スクリーンショットフック (TCC / screencapture を回避したGUI検証)

画面収録権限が無い環境でも、**アプリ自身のレンダリング結果を捕捉**できます。`GLINTVNC_SCREENSHOT=<保存先.png>` を指定してGUIを起動すると、遅延後 (`GLINTVNC_SCREENSHOT_DELAY_SEC`, 既定6秒) に egui viewport を PNG 保存して即終了します。ビューアの場合はフレーム到着後に捕捉します。

```bash
# Home 画面
GLINTVNC_SCREENSHOT=/tmp/home.png glintvnc
# 設定ウィンドウを開いた状態
GLINTVNC_OPEN_SETTINGS=1 GLINTVNC_SCREENSHOT=/tmp/settings.png glintvnc
# 実サーバへ接続した状態 (リモート画面込みで捕捉)
GLINTVNC_SCREENSHOT=/tmp/viewer.png GLINTVNC_SCREENSHOT_DELAY_SEC=14 glintvnc connect LAN-VNC
```

注意: 検証用フックであり、未指定なら一切動作しません。捕捉結果を issue/ドキュメントに貼る場合、接続先の画面に機密が写っていないことを必ず確認すること。

### リモートカーソル受信の診断

`GLINTVNC_DEBUG_CURSOR=1` を付けると、サーバから Cursor 擬似矩形 (-239) を受信した時点で stderr に `glintvnc: remote cursor WxH hotspot=(x,y)` が出る（画像の内容は出さない）。カーソル画像が届いているかを素早く見るためのフック。未設定なら何も出さない。

### 画面収録権限 (TCC) を使ったウィンドウ単位の捕捉 (2026-11-18 ユーザー付与済み)

権限がある環境では、`screencapture -l<windowID>` で GlintVNC のウィンドウだけを捕捉できます (デスクトップや他アプリの写り込みを防ぎます)。windowID の取得例:

```bash
swift -e 'import CoreGraphics; let l = CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as! [[String:Any]]; for w in l { if (w[kCGWindowOwnerName as String] as? String ?? "").lowercased().contains("glint") { print(w[kCGWindowNumber as String]!, w[kCGWindowName as String] ?? "") } }'
screencapture -x -o -l<windowID> /path/to/capture.png
```

**捕捉結果を検証に使ったら、必ず削除すること。** リモート画面にはユーザーの業務・個人情報が写り得ます。リポジトリ・issue・チャットへの貼り付けは厳禁です。

## 9. トラブルシューティング

| 症状 | 確認・対処 |
|---|---|
| "InsecureBlocked" で接続できない | プロファイルのセキュリティが `localhost_only` か。LAN先なら `profiles set X --security legacy` に明示切替 (平文。信頼できる網のみ) |
| "unsupported security method" | サーバが VeNCrypt/TLS 等を要求 (未対応)。サーバ側で VNC認証 (security type 2) を有効化するか SSH/WireGuard トンネル越しに接続 |
| "authentication refused" | パスワード違い (VNC認証はパスワードの先頭8文字のみ)。Keychain エントリを入れ直す (`secret delete` → `secret set`) |
| 画面が真っ黒 / 更新されない | サーバが Raw/CopyRect を喋れているか。デスクトップサイズ変更は DesktopSize 擬似エンコーディングで再取得 |
| キーが離れたままになる | フォーカス喪失時に自動 key-up します。それでも残る場合は接続を切って再接続 |
| フルスクリーンから戻れない | ツールバーの "Toggle full screen" または切断。Esc ではなく再接続で復帰可 |
| 設定が壊れた | `config.json.bak` から手動復旧 (上記§6)。壊れ方によっては一度リネームして再作成し、プロファイルを入力し直す |
| DMG からインストールできない | Gatekeeper: 右クリック→開く。署名はad-hocのためこれは仕様 (§4) |
| Keychain ダイアログが頻発 | ad-hoc 署名の cdhash 差異が原因のことがある。`secret set` し直す / 右クリック→開くを徹底する |

## 10. 依存関係の更新

- `cargo update` は小さく (1クレートずつ) 実施し、そのたびに `check.sh` と実機接続 (ローカルモック or 信頼できるテストサーバ) を通す
- ライセンス確認: 新規クレートを入れる場合は `cargo deny` (未導入なら手動で Cargo.toml の license フィールドを確認) と `docs/SECURITY.md` の方針に従う
- MSRV: stable でのみ検証する (rust-toolchain.toml)。古い macOS 13 の SDK との互換が失われる upstream 更新には注意
- `eframe` のメジャーアップは `App::ui` トレイト仕様など API が変わりやすい。アップデート時は `crates/glintvnc-app/src/ui.rs` のビルドエラーを解消してから機能差分を確認すること

## 11. アイコンの再生成

`assets/GlintVNC.icns` は `scripts/make-icon.py` (pure Python, 標準ライブラリのみ) で生成した原本を `iconutil` で変換したものです。
デザイナーが描く正式アイコンに差し替える場合:

1. 1024×1024 PNG を用意し、下記 iconset を作って `iconutil -c icns` で変換
2. `assets/GlintVNC.icns` を上書き → `bash scripts/package-macos.sh` を再実行 (DMGに同梱)

```bash
mkdir GlintVNC.iconset
for S in 16 32 128 256 512; do
 sips -z $S $S master.png --out GlintVNC.iconset/icon_${S}x${S}.png
 sips -z $((S*2)) $((S*2)) master.png --out GlintVNC.iconset/icon_${S}x${S}@2x.png
done
iconutil -c icns GlintVNC.iconset -o assets/GlintVNC.icns
```

## 12. 実機検証の記録方法

1. 必ずIntel実機 (macOS 13) で行う
2. 各操作の結果を `docs/VERIFICATION_LOG.md` に「日付・コマンド・実測結果・備考」で記録する。推測値を書かない
3. LAN上の実サーバを使う場合は、必ず所有者の許可を得てから。パスワードをドキュメントやログに書かない
4. セキュリティ上の制約: テスト用に Internet へ生の VNC ポート (5900等) を公開しない。必要なら SSH/WireGuard 越しに限定する

## 13. 既知の未解決課題 (2026-11-19 時点)

- TLS/VeNCrypt、Hextile/Tight エンコーディング未実装 (Raw + CopyRect + ZRLE + DesktopSize + Cursor + LED State を実装済み。ZRLE は接続単位の単一 zlib ストリーム・64x64 タイル・CPIXEL 3B で RFC 6143 準拠、収録・統合テスト済み)
- macOS は winit が Caps Lock を flagsChanged で完全に破棄するため**ローカル Caps Lock の検出は不可能** (egui::Modifiers に lock 系無し、双確認済み)。リモート Caps Lock の切替はセッション窓の浮動バー「CAPS」ボタン (KEYBOARD アイコン) で keysym 0xffe5 を送る。大文字テキストは Text 経路で常に正しいcaseで送られるため通常タイプは問題ない。リモートの lock 状態は LED State (-261) 対応サーバのみ浮動バーの CAPS ボタンが on 表示する (対応サーバのみ状態表示。非対応は盲トグル)
- 長押しリピートは rfbproto (client-side autorepeat、down の繰り返し + up exactly-once) に準拠。Do not filter repeat events
- Ctrl+click は MacToLinux/MacToWindows プリセットで RFB button3 (mask 0x04) に変換 (Cocoa/Finder 慣例)。Identity プリセットはそのまま (Control+button1)
- Phosphor アイコンフォント (egui-pop-phosphor 0.15, MIT/Apache-2.0 — egui 0.37 対応の公式系列。egui-phosphor 0.14 は egui 0.36 依存のため不適)。`.opencode/skills/frontend-design` に Anthropic のデザイン skill (Apache-2.0, 197k⭐) を導入し UI を本スキルの二段階プロセスで再設計
- ** 追記 (2026-11-19)**:
 - ドラッグ中はフローティングバーが自動で隠れる (`SessionShared.button_mask != 0` で描画・hover 再アームを停止)。バー矩形 (フレーム全域) 内の press/release/motion は fb に送られずローカル専用。バーボタンのクリックが fb の該当行へ二重配送されることはなくなった
 - フレーム更新はダーティ矩形のみを伝送・描画。フルフレームは connect/resize 時の1回のみ (1080p で 8.3MB)。ZRLE では更に圧縮される
 - LAN のドラッグがカクカクする根本の1つは**サーバ側の xfwm4 コンポジタが CopyRect を不可能にする**こと (redirected window では winPixmap≠screen pixmap、変更範囲が丸ごと再送される。tigervnc #908)。**実施済み (2026-11-19, ユーザー同意のもと SSH 経由で `xfconf-query -c xfwm4 -p /general/use_compositing -s false` を実行、読み戻し `false` を確認)**。CopyRect 再エンコードは発生しない想定。ドラッグはオーナー実機確認済み (2026-11-19、本番完了)
 - 全画面セッションが macOS の Space 切替で不安定になる事象を観測 (on-screen リストに出ず合成イベントが届かない; egui deferred viewport の native fullscreen Space との相互作用と推定)。再発時はセッションを閉じて再接続
 - Continuous Updates (-313 / message 150) は未実装。導入するなら既定OFFのオプトインで、rfbproto §ContinuousUpdates の手順 (`-313` 広告 → 初回 update 後 `EnableContinuousUpdates(1,0,0,w,h)` → 以後 incremental request を送らない → server message 150 を EndOfContinuousUpdates として受理) に従うこと
- **RFB Cursor 擬似エンコーディング (-239) 対応** (2026-11-18): サーバが対応すれば OS カーソル自体をサーバ画像に差し替える (`egui::Context::set_cursor_image` + CustomCursorImage、hotspot 一致)。未対応サーバは無言で無視されるため従来どおり fb 描画カーソルで動作。`GLINTVNC_DEBUG_CURSOR=1` で受信カーソルのサイズ/hotspot を stderr に出せる (画像内容・fb 内容は出さない)。実測ではこの LAN サーバ（x11vnc）は -239 を送出した（24x24 hotspot=(7,4)）
- ネイティブ NSEvent ベースのキーボード処理・日本語 IME・キーマップエディタ未実装 (egui 経由の入力のみ)
- フルスクリーン表示中の物理的配置 (別モニタ移動時の振る舞い) とウィンドウリサイズの連続操作は自動検証していない。接続は実サーバ (192.0.2.10) に対して成功済み (セッション描画・クリップボードは実測)。入力の効き具合は実機で確認すること
- **マルチウィンドウ構成**: 接続ごとに独立した子ビューポート (deferred viewport) が開く。ランチャーウィンドウ (root) を閉じると eframe の既定動作でアプリごと終了する。セッションウィンドウだけを閉じても他は継続。ドッククリック→ランチャー表示は winit 0.30 / eframe 0.37 に macOS dock-reopen イベントが存在しないため、「アプリ再起動→ランチャー」という挙動による (システム設定 > プライバシー ... ではなくコードレベルの制約)。恒久対応は隔離した AppKit デリゲート (unsafe) が必要 (P4 検討)
- **クリップボード共有 (テキストのみ)**: RFC 6143 §7.6.4/7.6.5 準拠 (msg 3 ServerCutText / msg 6 ClientCutText)。送受信ともに長さ上限 256KiB (TigerVNC `-MaxCutText` 相当)。接続直後にローカルクリップボードを無条件送信しない (RealVNC `SendInitialClipboard=false` 相当)。エコー抑制あり。**内容をログ・ステータス・設定ファイルに出さない**。拡張擬似エンコーディング (0xc0a1e5ce) の UTF-8 拡張は未実装。Latin-1 厳密解釈のサーバ (RealVNC) との日本語クリップボード互換は未保証
- ad-hoc 署名のみ配布可。公証なし。**ad-hoc ビルドを作り直すたびに cdhash が変わり、Keychain 項目の ACL ("このアプリにアクセスを許可") が外れることがある**。接続時に "glintvnc が項目にアクセスしようとしています" のダイアログが出たら「常に許可」を押すか、`glintvnc secret delete <名>` → `glintvnc secret set <名> --stdin` で入れ直す。Developer ID 配布をしない方針の限りこれは恒久的制約 |
- 入力経路は統合テストでバイト列検証済み (KeyEvent/PointerEvent)。実サーバへの実入力もオーナー実機確認済み (2026-11-19、本番完了)
- 複数ウィンドウの描画同期 (vsync の直列化、egui #5836) は未計測。2窓以上でのフレームレート劣化が疑われる場合は deferred viewport への完全移行を検討
- Screen Recording 権限 (TCC) は 2026-11-18 ユーザー付与済み。`screencapture -l<windowID>` で GlintVNC ウィンドウのみ捕捉できる。ただし**リモートデスクトップの内容を含むキャプチャは個人情報を含みうるので、検証後に必ず削除し、絶対にリポジトリや issue にコミットしないこと**

## 14. 緊急時連絡・変更履歴

保守体制はオーナー (リポジトリ所有者) に従う。このファイルと `docs/VERIFICATION_LOG.md` を変更の都度更新すること。

| 日付 | 変更 |
|---|---|
| 2026-11-18 | 初版作成 (P0/P1 相当の検証完了 + P2 の一部: CopyRect, 中ボタン, 1:1/Fit/Stretch, 統合テスト追加) |
| 2026-11-18 | UI全面改修: パネル構成 (chrome/status/left-profiles/right-session)、設定 Window (言語 ja/en・テーマ dark/light/system)、スクロール速度0.25–4.0×、日本語フォント (Hiragino W3/W6)、文字色 #FFFFFF/#000000、透過フレームバグ修正、`GLINTVNC_SCREENSHOT` フック追加、config v1 に `scroll_speed` 後方互換追加 |
| 2026-11-19 | ドラッグ破綻と遅延の完全改善 (フローティングバーはドラッグ中 `button_mask!=0` で非表示・バー矩形内の press/release/motion を fb に送らずローカル専用化し二重配送を根治。フレーム更新をダーティ矩形ストリームへ全面書き換え (`Event::Frame{rects}`、全量コピー廃止・永続 ColorImage へ直接パッチ・GPU は `set_partial` で矩形のみアップロード)。セッション窓の無条件 16ms 再描画を廃止 (バー表示中の自動隠しタイマー分のみ)。ZRLE(16) クライアント実装 (接続単位の単一 zlib ストリーム、64x64 タイル、固定 CPIXEL=3B、RFC 6143 準拠、SetEncodings 先頭配置)。調査の副産物: xfwm4 コンポジタ下では Xvnc が CopyRect を送出しない (XRender フック、tigervnc #908) → リモートでのコンポジタOFF (`xfconf-query -c xfwm4 -p /general/use_compositing -s false`) を推奨。Continuous Updates (-313) は UltraVNC 等の未対応サーバで message 150 が fatal になりうる互換リスクと LAN RTT 実利の小ささから見送り)。当時の DMG: SHA256 ce0150a529cac0effbb8ea264119475af99ff430896dd53e54b5b1db28f09be7 |
| 2026-11-19 | Input pipeline: 入力パイプライン強化 — コマンドチャネルを unbounded 化 (境界128の channel + try_send は満杯時に pointer release 自体を silent drop し得た)。ポインタ move は UI で 16ms 間引き + latest-wins pending スロットに畳み込み (MotionThrottle 純関数、単体テスト4件)。ボタンマスク変化は即時送信、press/release 前に pending を必ず flush。worker の read_update は**矩形の処理ごとに**キュー済みコマンドを即送信 (巨大更新のデコード中に入力が1件も届かなかった構造的問題を解消)。フォーカス喪失時の button release も never-drop 経路。`GLINTVNC_DEBUG_FRAMES=1` にエンコーディング別矩形カウント (raw/copyrect/zrle/desktopsize/cursor/led) を追加。LAN 実機 (ウィンドウモード、スクラッチ設定で全画面無効化→復元): フレームバッファ描画 PASS、ライブ統計で ZRLE 未移動矩形を実測・ローカル確認後削除。**リモートの xfwm4 コンポジターをユーザー同意のもと SSH で無効化** (`xfconf-query -c xfwm4 -p /general/use_compositing -s false`、読み戻し false)。44 テスト green |
| 2026-11-19 | パッケージ: | PASS | `dist/GlintVNC-0.1.0-macOS-x86_64.dmg` (SHA256 4d9f58412311e629259473784c2ce49a8af054bda5a0d788c5a5daf4030fe72b) — ad-hoc signed; `glintvnc connect LAN-VNC` 実機起動→ウィンドウモードでリモートデスクトップ描画を確認（キャプチャは確認後削除）。全画面 Space の不安定さは既知課題のまま。cdhash 再変化のため Keychain 再許可が必要 |
| 2026-11-18 | 入力完全化 (PointerButton イベント駆動 + ドラッグのfb端clamp streaming、Ctrl+click=右クリック(MacTo*)、長押し=rfbproto準拠クライアントリピート、F1–F35/Insert/PrintScreen/Pause/Menu/CapsLock/NumLock/ScrollLock keysym化、LED State疑似符号(-261)対応でCAPS状態表示) + UI再設計 (Phosphor regular icon、ブランドマーク、セクション分割フォーム、セグメント制御、表示/リモート/このアプリ設定グループ化、frontend-design スキル(Apache-2.0, anthropics/skills) 導入 — 内容を目検して安全確認の上採用) |
| 2026-11-19 | オーナーによる実機試用完了。物理 Intel Mac で v1.0.0 DMG を実際に操作し、本番環境完了を宣言。Drag / F8 / Esc / 再描画 / Keychain 承認の手動確認が完了（Editor 側の GUI 操作は行わない方針を維持）。README / README_JA / VERIFICATION_LOG / DESIGN_ROUND8 / 公開保守メモ / 紹介サイト4言語 / Release notes / プライベート保守メモを最終更新 |
| 2026-11-19 | 全ソース精査（4並列監査: メモリ/クラッシュ安全性・安定性/機能確実性・GUI/設定整合性・VNC互換）。修正: CopyRect flood による OOM（更新あたりRGBA 128MiB上限 + キャンバス16Mpx）、ネットワーク読み取り無期限ブロック（15秒タイムアウト + JoinHandle 保持/reaper）、切断時に古いフレームが残り再接続UIが出ない問題、スクロールがドラッグ中のボタンを解放する問題、UIスレッドでのKeychain同期呼び出し、DesktopSize の仕様違反（次回要求を非増分化）、カーソルhotspotの符号付き解釈とクランプ、LibVNCServer の LED State (0xFFFE0000) 対応、パスワード保存時のNone認証への黙ってのダウングレード禁止、同名プロファイルの拒否、F8/Esc のキーボード経路、GUI でのスケール/キーマップ設定、i18n 未使用キー削除とスキャンテスト、クリップボード既定値の opt-in 化、request_repaint の viewport 指定。55テスト green。DMG SHA256 c13759fa2ee7b0fd612399719d19eac24ffa9bdec7dfe1c4f16caf744ed8b58c。スクラッチ設定で実機 LAN probe PASS（Connected: test-server:0 1920x1080）。詳細 `docs/DESIGN_ROUND8_AUDIT.md` |
| 2026-11-19 | 残像・ラグ調査（サーバは x11vnc と判明、物理 display :0、`-forever -shared -usepw -bg -noscrollcopyrect`）＋クライアント対策。浮動バーに「再描画 (Refresh)」ボタン（ARROWS_CLOCKWISE）＋ ⌘R で非増分 FramebufferUpdateRequest を発行しサーバ全画面を再送（残像即消去）。SetEncodings に CompressLevel 1（-255）を追加（x11vnc/LibVNCServer はクライアントの CompressLevel を尊重、エンコード CPU 負荷を低減）。統合テスト refresh_triggers_full_framebuffer_request 追加（非増分 request を byte 検証）。45 tests green。DMG: SHA256 de8e27ecb9ecdad2764864f176af38c5ef70eea058a9e4c5bdad466c3b52f158。ドラッグ体感はオーナー実機確認済み (2026-11-19、本番完了)。サーバ側チューニング（wait/defer 短縮、-xrefresh）はユーザー判断（live なデスクトップはこちらから再起動しない） |
| 2026-11-18 | 遅延根治 (protocol→UI の起床をイベント駆動化; root 250ms ポーリング起因の描画滞留を解消、Frame 毎に request_repaint_of)、Pointer 座標の仕様適合化 (round→clamp)、押下ボタンの解放リーク修正 (画像外/フォーカス喪失/切断時に buttons=0)、RFB Cursor 擬似エンコーディング (-239) 実装で OS カーソルをサーバカーソル画像へ差し替え (1920x1080 LAN 実機で 24x24 hotspot=(7,4) 受信を実測)、GLINTVNC_DEBUG_CURSOR 診断フック追加 |
| 2026-11-18 | 黒画面の根治 (増分フレームをゼロキャンバスに適用していたクライアント側バグ。canvas永続化+clone送信、退行テスト追加)、セッションを独立子ウィンドウ化 (マルチウィンドウ同時接続、deferred viewport、全画面フルリモート、下部パネル撤廃、ホバーで出る自動隠れ制御バー)、接続時フルスクリーンはプロファイル `fullscreen_on_connect` に準拠 (接続ごとに ViewportCommand::Fullscreen)、ドッククリック=アプリ再起動→ランチャー (winit/eframe に reopen イベントなし)、クリップボード双方向共有 (msg 3/6, UTF-8/Latin-1フォールバック, 256KiB上限, echo抑制, `clipboard_sync` 設定+CLI)、フォント微調整 (Hiragino y_offset_factor=0.08) |
