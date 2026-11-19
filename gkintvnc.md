# GlintVNC 保守メモ（公開）

このファイルは共通ルール（ルール6）に従い、今後の開発向けの備考を集約するものです。
詳細な保守情報・非公開メモは、プライベートの `oriyu90/common-rules-document` リポジトリ
（`gkintvnc.md`）で管理します。ここには秘密情報を記載しません。

## 現状（v1.0.0 / 2026-11-19）

- 公開リポジトリ: https://github.com/oriyu90/gkintvnc
- Release: https://github.com/oriyu90/gkintvnc/releases/tag/v1.0.0
- 紹介サイト: https://studio-rizi.pages.dev/projects/gkintvnc/
- DMG は **ad-hoc 署名・未公証**です。初回起動は右クリック→「開く」、
 または「システム設定 → プライバシーとセキュリティ」での許可が必要です。
- 再ビルドごとに cdhash が変わるため、Keychain の許可を再度求められる場合があります。

## 今後の作業（未実装の機能）

- TLS / VeNCrypt / Apple ARD / RA2、Tight / Hextile、Continuous Updates / Fence、
 ExtendedDesktopSize、IME の完全対応は未実装（意図的な先送り）。
- 実機 GUI の手動確認はオーナーにより完了（2026-11-19）。本番環境完了を宣言済み。
- 4K / 5K の長時間負荷測定、fuzz テスト、`cargo audit` / `cargo deny` は未実施。
- Developer ID 署名・notarization へ移行する場合は entitlements を別途レビューする。

## バージョン更新時に触れる箇所

1. `Cargo.toml` の workspace version、`assets/Info.plist`（`package-macos.sh` が同期）
2. `README.md` / `README_JA.md` のバージョン・リンク・検証結果
3. `docs/VERIFICATION_LOG.md` のテスト件数と DMG SHA256
4. GitHub Release のタグ・ノート・添付 DMG
5. studio-rizi の `website/content.js`（PROJECT / RELEASE 情報）と各言語ページ
 （`npm run build` → `validate` → `count-files` → `npm test`）
