# GlintVNC（日本語ガイド）

**対象:** macOS Ventura 13.7.8 / MacBook Pro 2017 Intel / x86_64。

**著作者:** Yuki_Orita（折田悠希 / おりたゆうき） · **ライセンス:** MIT

詳細な保守情報はプライベートの `oriyu90/common-rules-document` リポジトリ（`gkintvnc.md`）で管理します。

このリポジトリは、Rust 製のネイティブ VNC ビューアと、その検証環境一式です。
詳細な検証結果は `docs/VERIFICATION_LOG.md`、設計は `docs/ARCHITECTURE.md`、
セキュリティ要件は `docs/SECURITY.md` を参照してください。

## ビルドと検証

```sh
bash scripts/bootstrap-macos.sh      # 環境調査（ソフトを勝手に入れない）
bash scripts/check.sh                # fmt / check / test / clippy
bash scripts/package-macos.sh        # dist/GlintVNC-0.1.0-macOS-x86_64.dmg を生成
```

`package-macos.sh` は.ad-hoc 署名の `.app` と DMG を作ります。配布用には
Developer ID 署名と notarization が必要です（現状は未対応、Gatekeeper の
初回起動は右クリック→「開く」）。

## 実装済みの機能（2026-11-19 現在）

- RFB 3.3 / 3.7 / 3.8、VNCAuth、Raw / CopyRect / ZRLE、および Cursor / DesktopSize / LED State
  擬似エンコーディング
- 差分矩形ストリーミングと `TextureHandle::set_partial` による部分 GPU 転送
- 16ms のポインタ移動スロットル、ボタン解放の確実な配送、矩形間の入力送信
- 浮動バー（接続一覧 / 全画面 / CAPS / 再描画 / 設定 / 切断）、F8 で表示切替、
  Esc で全画面解除、⌘R で強制再描画
- Keychain にのみ保存するパスワード、GUI と CLI で共通の設定 JSON
- 設定: 言語 ja/en、テーマ dark/light/system、スクロール速度、
  クリップボード共有（既定オフ）、表示スケール、キーマップ
- CLI: `profiles` / `settings` / `secret` / `probe` / `connect`
- ネットワーク読み取りのタイムアウト、累積ダメージ上限、
  切断時のボタン・キー状態の解放、再接続（ジッタ付きバックオフ）

未実装（明示的に先送り）: TLS/VeNCrypt、ARD/RA2、Tight/Hextile、
Continuous Updates、Fence、IME の完全対応。詳細は
`docs/COMPATIBILITY_MATRIX.md` を参照してください。

## よくある勘違い

- VNCAuth は通信路を暗号化しません。LAN 使用はプロファイルで
  「レガシー（暗号化なし）」を明示した場合のみ許可されます。
- 画面が見えることと、低遅延・低 CPU であることは別です。実機測定は
  `docs/VERIFICATION_LOG.md` に記録しています。
- ドラッグ操作や Keychain の「常に許可」など、実機でしか確認できない項目は
  ユーザー自身の確認が必要です（Editor 側からの GUI 操作は行いません）。
- ad-hoc 署名の再ビルドでは cdhash が変わるため、Keychain の許可を
  再承認する必要があります。

## ドキュメント

| ファイル | 内容 |
|---|---|
| `AGENTS.md` | 作業規則（必読） |
| `OPEN_CODE_START_PROMPT.md` | 初期依頼の原文 |
| `docs/ARCHITECTURE.md` | 設計・データフロー |
| `docs/SECURITY.md` | 脅威モデルと秘密情報の扱い |
| `docs/PRODUCT_SPEC.md` / `docs/ROADMAP.md` | 機能一覧とロードマップ |
| `docs/QA_PLAN.md` | テストケース |
| `docs/VERIFICATION_LOG.md` | 実機検証の証跡 |
| `docs/DESIGN_ROUND*.md` | 各ラウンドの調査・修正記録 |
| `docs/research/` | 参考アプリ・ライブラリ・出典 |
