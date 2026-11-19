# GlintVNC

**Intel x86_64 Mac（macOS 13 Ventura 以降）** 向けのネイティブ VNC ビューアです。Rust で実装し、日本語と英語の UI に完全対応しています。

- **著作者:** Yuki_Orita（折田悠希 / おりたゆうき）
- **ライセンス:** MIT — 全文とサードパーティライセンスは [LICENSE](LICENSE) を参照
- **リポジトリ:** https://github.com/oriyu90/gkintvnc
- **リリース:** https://github.com/oriyu90/gkintvnc/releases/tag/v1.0.0
- **紹介サイト:** https://studio-rizi.pages.dev/projects/gkintvnc/

## できること

RFB/VNC デスクトップへ接続し、リモート画面を専用ウィンドウ（または全画面）に表示します。RFB 3.3 / 3.7 / 3.8 と VNCAuth に対応し、Raw / CopyRect / ZRLE エンコーディングを扱います。画面更新は小さな差分矩形としてストリーミングし、GPU へ部分是転送するため、ウィンドウのドラッグも滑らかに動きます。

UI は**日本語・英語の両方に完全対応**しています。接続一覧、プロファイル編集、設定、セッション操作バーまで同じ機能が使えます。表示スケール（全体表示 / 原寸 / 拡大）とキーマップは接続ごとに設定できます。操作バーには残像を消す「再描画」（⌘R）、バーの表示切替（F8）、全画面解除（Esc）があります。

パスワードは macOS Keychain にのみ保存し、設定 JSON・ログ・コマンドライン引数には書きません。リモートの画面データは信頼できない入力として扱い、厳しい境界チェック、更新あたり 128 MiB の RGBA 上限、16 Mpx のフレームバッファ上限、読み取りタイムアウトで保護しています。

## ダウンロードと初回起動

[v1.0.0 リリース](https://github.com/oriyu90/gkintvnc/releases/tag/v1.0.0) から `GlintVNC-1.0.0-macOS-x86_64.dmg` をダウンロードし、マウントして Applications へドラッグしてください。

DMG は **ad-hoc 署名であり、Apple 公証は行っていません**。初回起動は右クリック→「開く」、または「システム設定 → プライバシーとセキュリティ」で許可してください。再ビルドごとに署名が変わるため、Keychain が再度許可を求める場合があります。

## ソースからビルド

```sh
bash scripts/check.sh          # 整形・チェック・55テスト・clippy
bash scripts/package-macos.sh  # ad-hoc 署名の .app と DMG を作成
```

Rust 1.100 以降（`x86_64-apple-darwin` ターゲット）と macOS 13 SDK が必要です。

## コマンドライン

同じバイナリで `profiles` / `settings` / `secret` / `probe` / `connect` サブコマンドが使えます。詳細は `glintvnc --help` を参照してください。

## サポート

バグ報告・告知: [Discord](https://discord.gg/x7KXhNTD8M) · [GitHub Issues](https://github.com/oriyu90/gkintvnc/issues) · [X](https://x.com/InovateofRIZI) · [Studio Rizi](https://studio-rizi.pages.dev/)
