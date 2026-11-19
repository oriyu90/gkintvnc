# UI / 設定画面 設計案 (2026-11-18)

対象: GlintVNC 0.1.0 / Intel MacBook Pro 2017 / macOS 13.7.8 / eframe 0.37 + egui 0.37

## 0. 前提と制約 (現状把握の結果)

- `AppSettings` には `theme` (dark/light, CLIは system も許容) と `language` (ja/en) が既にあるが、GUI は Home の "Appearance" 行に theme ボタンが直書きされ、language は GUI 未反映。
- `default_fullscreen` はストアされているが GUI 未消費 (現状維持)。
- ホームは `ui.columns(2, …)` の等幅2列で左右対称にならざるを得ず、フォームはラベル縦積みで余白が偏る。ビューアはツールバー1本に機能が詰まり、ボトムにも status が二重表示。
- ウィンドウは 1110×720 固定・最小サイズなし。リサイズ時に列が潰れ、ラベルが切れる。
- 日本語フォント未ロードのため、日本語UIは tofu (豆腐) になる。
- リポジトリに UI/UX 系 Skill は存在しない (`.opencode/` 自体が無く、`docs/research/LIBRARIES_AND_SKILLS.md` が参照する `ui-polish` も未同梱)。このため本設計は、汎用エージェントによるUIレビュー (2026-11-18) と PRODUCT_SPEC.md の要件を根拠にした。
- PRODUCT_SPEC: 「Japanese-first UI」「Dark/light/system」「minimal graphite palette and accent blue/teal」「no crowded toolbar」「GUI/CLI update the same model」。
- ユーザーからの明示要件: 背景黒→文字 ffffff、背景白→文字 黒。接続時カスタムは押しやすい専用エリアに。スクロール速度調整。言語・ダークモード切替の設定画面。

## 1. 情報構造 (パネル配置)

```
┌────────────────────────────────────────────────────────────┐
│ TopBottomPanel::top GLINT | VNC · macOS Intel [＋新規] [⚙]│ 全ビュー共通ヘッダ
│ (viewer時: [◀ 接続一覧] … [接続設定][全画面][再接続][⚙])
├──────────┬─────────────────────────────────┬───────────────┤
│ SidePanel│ CentralPanel │ SidePanel │
│ left │ Home: プロファイル編集フォーム │ right │
│ 264px │ Viewer: リモート画面 (Fit/1:1) │ 232px │
│ 保存済み接続│ Settings: 中央モーダル相当Window │ 接続設定 (viewer│
│ (Homeのみ)│ │ のみ) │
├──────────┴─────────────────────────────────┴───────────────┤
│ TopBottomPanel::bottom status (1行) [解像度|ver] │
└────────────────────────────────────────────────────────────┘
```

- ウィンドウ: 既定 1180×760、最小 880×580。
- status はボトムバーに一本化 (ツールバーの status ラベルは廃止)。

## 2. Home

- 左パネル: 見出し「保存済み接続 (n件)」+ ScrollArea でカード列挙。カード = 名前(太字16px) + host:port + [▶ 接続 (primary)] [編集]。選択中カードは accent で枠線強調。空状態は「まだありません → 右のフォームで作成」+ 作成ボタン。
- 中央: 最大幅620px・水平中央。`egui::Grid::new(2列)` で右寄せラベル(110px) + フィールド。`ScrollArea::vertical` で高さ不足時にスクロール。
 - 項目: 名前 / ホスト名・IP / ポート / ユーザー名 / パスワード (Keychainのみ・空欄=変更なし) / 自動再接続 / 接続後に全画面。
 - セキュリティ: `Localhost only` / `Legacy (暗号化なし)` の2ボタン + Legacy選択時は accent枠の警告ボックス。
 - ボタン行は右寄せ: [保存する primary] [削除する (2段階確認・danger)]。New はヘッダの「＋新規」に移動。
- "Appearance" 行は廃止 → 設定画面へ。

## 3. Viewer (接続中)

- ヘッダ: [◀ 接続一覧] ・ status ドット+テキスト ・ 右側 [接続設定パネル表示切替] [全画面 toggle] [再接続] [⚙ 設定]。
- 右パネル「接続設定」 (resizable, 既定232px, 最小200):
 1. **表示**: [全体表示 Fit] [原寸 1:1] [拡大 Stretch] 縦並び大型セレクト (高さ38px)。
 2. **スクロール速度**: スライダー 0.25×–4.00× (step 0.25) + 現在値 + [リセット]。即時反映。
 3. **セッション**: [全画面切替] [再接続] [切断 (danger, 最下段に分離)]。
 - 注記: 表示モードはプロファイルに保存・スクロール速度は全体設定、の1行。
- リモート描画: Fit=中央寄せ+letterbox、1:1=ScrollArea両方向、Stretch=availability全域。接続中は spinner +「キャンセル」。

## 4. 設定 (⚙) — `egui::Window` 中央配置・全ビューから開ける

- **言語**: [日本語 | English] 即時反映 + 永続化。
- **テーマ**: [ダーク | ライト | システムに合わせる]。egui の `set_visuals_of` で両テーマの visuals を登録し `ThemePreference` を System/Dark/Light で切替。即時反映。
- **スクロール速度**: 上記と同一設定値のスライダー。
- **このアプリについて**: バージョンと ad-hoc 署名の注意書き (日本語)。
- パスワード/秘密情報は一切含まない。

## 5. 配色・文字色 (視認性要件)

| 要素 | Dark (bg ≈ #0E0E10, window #17171B) | Light (bg #FFFFFF) |
|---|---|---|
| 本文テキスト | **#FFFFFF** | **#000000** |
| アクセント | #58A6FF 系 blue | #0969DA 系 blue |
| danger/警告 | #FF7B72 (accent枠警告ボックス) | #CF222E |
| カード/ホバー fill | #1C1C21 / #22222A | #F3F3F6 / #EBEBEF |

`Visuals` の全 widget の `fg_stroke` を #FFFFFF / #000000 に統一 (ボタン・ラベル含む)。セマンティック色 (警告/エラー/リンク) のみ個別色を維持。

## 6. 日本語フォント

- 起動時に macOS 標準の `/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc` (本文) と `W6.ttc` (太字) を `FontDefinitions` に挿入し、Proportional/Monospace のフォールバックに追加。読込失敗時は既定フォントにフォールバック (Latinのみ)。
- tofu 防止の実機確認を検証ログに記録する。

## 7. スクロール速度の実装

- `AppSettings.scroll_speed: f32` (default 1.0, clamp 0.25–4.0, CLI `settings get/set scroll_speed` 対応済)。設定は v1 JSON に serde default で後方互換。
- GUI は `MouseWheel` の `delta` を `scroll_speed` で乗算して累積し、28pt ごとに VNC スクロールボタン (4/5/6/7) パルスを送出 (ボタン4=8, 5=16, 6=32, 7=64)。符号が変わったら累積をリセットしない (方向別アキュムレータで混線防止)。1イベントあたり最大6パルスに制限。

## 8. 画面全体の自己検証フック (開発/検証用)

- 環境変数 `GLINTVNC_SCREENSHOT=<path.png>` を指定すると、遅延後 (`GLINTVNC_SCREENSHOT_DELAY_SEC`, 既定6秒) に **egui viewport のレンダリング結果を自分で PNG 保存** し、即座に終了する。TCC/screencapture に依存しないため、GUI 描画の実測 (日本語フォント・レイアウト・接続後のリモート画面) をこのシェルからも取得できる。
- `GLINTVNC_OPEN_SETTINGS=1` で起動時から設定 Window を開いた状態にできる。
- 検証専用・本番動作に影響なし (環境変数未指定時は無害)。`docs/MAINTENANCE.md` に手順を追記する。

## 9. 検証計画

1. `cargo fmt` / `cargo test --workspace` / `clippy -D warnings` / `scripts/check.sh`。
2. スクリーンショットフックで Home・設定画面・(モックサーバへ接続した) ビューアの3枚を捕捉し、目視確認 (レイアウト崩れ/日本語豆腐/文字色)。
3. `probe` + GUI スクリーンショットで 192.0.2.10:5900 への実接続・リモート描画を確認。入力は統合テストのバイト検証 + ユーザーによる実機操作確認。
4. 結果を `docs/VERIFICATION_LOG.md` に記録。

## 10. スコープ外 (今回やらない)

- TLS/VeNCrypt, Tight, ネイティブNSEvent入力/IME (P3/P4)。なお ZRLE ・クリップボード・自動隠しバーは 2026-11-19 に実装済み。F8メニューは未実装。
- Developer ID 署名・公証 (ユーザー方針: しない)。
