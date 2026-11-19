# GlintVNC ラウンド4設計案 — 入力実装の完全化 + UI 再設計 (2026-11-18)

対象ユーザー報告 (2026-11-18):
1. マウス: クリック中の移動（ドラッグ）、右クリックなどの判定が不完全な実装部分を調査し実装する。
2. キーボード: キーの長押し、Caps Lock などの判定が不完全な実装部分を調査し実装する。
3. UI（ボタン含む）を全体的に見やすく・きれいに配置する。デザイン系 Skill を GitHub から調査・導入して実践する。

## 0. 調査で確定した事実 (ローカルレジストリ)

### egui 0.37 イベント体系（ソース実測）
- `egui::Event::PointerButton { pos, button, pressed, modifiers }` — **押下/解放の離散イベント**が取れる（`PointerButton::{Primary, Secondary, Middle, Extra1, Extra2}`）。winit の Left/Right/Middle/Back/Forward に対応。
- `egui::Event::Key { key, physical_key, pressed, repeat, modifiers }` — **`repeat: bool` があり、macOS の autorepeat は egui が検出して `repeat: true` を付けて流してくる**（egui-winit の `on_keyboard_input` 経由）。
- `egui::Modifiers` は **shift/ctrl/alt/mac_cmd/command のみ**。Caps/Num/Scroll Lock は Modifiers では取れない。
- `egui::Key` には **CapsLock, NumLock, ScrollLock, PrintScreen, Pause, Menu, Fn** と、F1–F35、句読点（Backtick/Semicolon/Quote 等）、ShiftLeft/Right、ControlLeft/Right 等の物理キー種別がある。winit macOS は `KeyCode::CapsLock → Key::CapsLock` を生成する（winit-0.30.13 macos/event.rs:534）。
- winit macOS でも Caps Lock キーコード変換は存在するが、**macOS 本体が Caps Lock を flagsChanged として処理し、かつ入力メソッド/等幅スクリーンショット環境では KeyboardInput が来ない場合がある** — 不確定。egui に届くかは実機でしか確認できない。設計方針: 「届いたら転送する（best-effort）」＋「届かない場合は仕組み上の制約として明記」。

### 現行実装の不完全さ（把握）

| 症状 | 原因 |
|---|---|
| ドラッグが画像端/レターボックス/フローティングバーに入ると途切れる | `release_buttons` を pointer 離脱時に呼ぶ実装（Round 3）が、ボタン押下中の `PointerEvent buttons=0` を強制送信するため。**押下中は fb 座標を clamp して送り続けるべき**。 |
| 右クリックの扱い | egui は `secondary` = 物理右ボタンにマップ。Mac の慣例「Ctrl+クリック＝右クリック」への変換がない（TigerVNC の挙動は調査中）。 |
| キーの長押し | `repeat: true` の Key イベントを検証していない。macOS では long-press 中に repeated Key イベントが届くが、i) サーバ側 autorepeat (XTEST) と client-side repeat の二重押しになるか、ii) Text 経路だけが送られサーバ autorepeat に依存する。仕様に沿って整理する（調査中）。 |
| Caps Lock/Num Lock/Scroll Lock | `Key::CapsLock` 等は egui/winit がマップ可能だが、現行コードの keysym 対応表に存在しない → 何も送られない。 |
| F1–F24 | egui の Key::F1–F35 を現行コードは未マップ → **生の F キーは何も起きない**（Text 経路にも乗らない）。 |
| 修飾キー無しの句読点 | Text 経路で届くので入力はされるが、Ctrl 等が押された時は Text が出ず何も送られない |

### UI の現状（before 画像は `/private/.../ui-before.png` 保存・確認済み・削除）
- 余白リズムは悪くないが、ヒエラルキーが弱い。接続ボタンだけが accent 塗り、他が無個性。ヘッダーは「GLINT」+tagline で余白過多。左カラムのカードは情報が薄い（名前＋ホストのみ）。フォームはラベル右揃えで古い印象。空状態・エラー状態の指針なし。アイコンなし。

## 1. デザイン Brief（LAN 管理者の精密道具として）

**サブジェクト**: 業務端末の Linux/Windows デスクトップへ Mac から SSH 並みの気軽さで入るための VNC ランチャー + リモートビューア。**ユーザー**: リポジトリ所有者本人（＋同様の管理者）。**主目的**: 「保存済み接続を1クリック/1キーで開き、画面がそのまま手元にある」ことを最速で行う。道具の性格は「KVM スイッチのような精密機器 + ターミナルの気軽さ」で、派手な装飾や SaaS 風カードは要らない。

### トークン（ダーク）

| 役割 | 値 |
|---|---|
| bg / window | `#0E1014` |
| panel (left/top/bottom chrome) | `#13151A` |
| raised (cards, inputs) | `#1A1D24` |
| hover | `#21252E` |
| divider (hairline) | `#262B36` |
| text (fg) | `#E8EAF0` |
| text dim | `#9AA3B2` |
| accent (primary action) | `#4C8DFF`（予備: hover `#5E99FF`） |
| danger | `#F0554D` |
| ok (connected dot) | `#3FB97F` |
| radius | 10px (cards) / 8px (buttons, inputs) |
| spacing unit | 8px（カード内 14-16px、ヘッダー 8-12px） |
| type scale | 22 / 15 / 13 / 11（Hiragino W3 body, W6 for strong） |

ライトテーマも同値を反転させた対気配で定義（bg `#FAFAFC`, panel `#F3F3F6`, card `#FFFFFF`, divider `#E4E4EA`, fg `#16181D`, dim `#5B6470`, accent `#0B69DA`, danger `#CF222E`, ok `#1A7F4B`）。

### レイアウト

```
┌─────────────────────────────────────────────────────────────┐
│ [glyph] GlintVNC 0.1.0          [＋ 新規] [⚙ 設定]            │  ← 44px, no tagline
├──────────────┬──────────────────────────────────────────────┤
│ 保存済み接続   │  新規接続 / {name} を編集                    │
│ ┌──────────┐ │  ┌─────────────────────────────────────────┐ │
│ │ icon name│ │  │ 接続情報 ───────────────────────────    │ │
│ │ host:port│ │  │ 名前 [____________________]              │ │
│ │ ● 接続中 x│ │  │ ホスト [____________________]  ポート [_] │ │
│ │ [接続▶][⋯]│ │  │ ユーザー名 [____________________]        │ │
│ └──────────┘ │  │ パスワード [____________________]          │ │
│ ┌──────────┐ │  │ ───── オプション ───────                │ │
│ │          │ │  │ [x] 接続後に全画面  [x] 自動再接続        │ │
│ └──────────┘ │  │ ───── セキュリティ ────────              │ │
│ (empty state)│  │ ( ) ローカルのみ  ( ) レガシー           │ │
│              │  │                    [削除] [保存して接続]  │ │
│              │  └─────────────────────────────────────────┘ │
├──────────────┴──────────────────────────────────────────────┤
│ ● 2 件接続中                            GlintVNC 0.1.0        │  ← 28px
└──────────────────────────────────────────────────────────────┘
```

- 左カラム 280px。カード = モニタアイコン（phosphor `DESKTOP`）＋名前（W6 15px）＋ `host:port`（mono, dim 12px）＋右上ステータス ● 接続中 n（accent）。下段に [接続] primary + [編集] ghost（pencil icon）。
- 右ペインは常にフォーム。保存ボタンは accent 塗り・文言は「保存する」固定。削除は ghost + danger text。新規ボタン（header）は `PLUS` icon + 新規。
- フォームをセクション区切り（接続情報 / オプション / セキュリティ）でグループ化。
- 設定: セクションは 表示 / リモート接続 / このアプリ の3グループ、行はトグル or セグメント、ラベル＋説明 dim 小文字。
- セッション窓のフローティングバー: icon + 短ラベル（← 接続一覧 / 全画面 / 設定 / 切断 power danger）10-12px。
- 空状態: 中央、phosphor `PLUG` アイコン 48px dim + 「まだ接続がありません」+「左の一覧から接続を選ぶか、ここで新規接続を作成してください」。
- ステータスバー: 左=コンテキスト文（ready/接続中…）、右=● N 件接続中 pill（accent dot）+ バージョン dim。
- ボタン: 共通高さ 32-34px, radius 8px, horizontal padding 14px, Man rope 相当の字送り無し。階層: Primary(accent fill) / Secondary(card fill + hairline stroke) / Ghost(no frame) / Danger(danger fill only when armed)。
- 全画面アイコンは `ARROWS_OUT_SIMPLE` / `ARROWS_IN_SIMPLE`、戻るは `ARROW_LEFT`、設定は `GEAR`、切断は `POWER`（danger tint）、保存は `CHECK`、削除は `TRASH`、編集は `PENCIL`。
- **AI 量産風の回避**（skill §calibration）:
  - 近黒＋蛍光 accent は避ける（accent は落ち着いた青 `#4C8DFF` に）。
  - SaaS カードの「全カード同一 radius + 同一ソフトシャドー」は避ける（radius は hierarchy 別、影はハイライトラインのみ、影より hairline stroke）。
  - ALL-CAPS eyebrow / `A · B · C` メタ / em dash ラベル / 末 tail の → は使わない。
  - タイプスケールは 4 段のみ。アイコンは Phosphor regular に統一（emoji は使わない）。
- フォント: Hiragino W3/W6 を現行どおり。コード部分は monospace（`Menlo` 名を font family に追加）。
- モーション: 新規/保存時にカードへ1回だけ flash（200ms, hover fade のみ・入場アニメは無し）。

## 2. 入力タスク（実装計画）前提

以下を確定後に破棄/実装する（research 中断に備え設計を分ける）:

### 2-1. Pointer（ドラッグ破綻の根治）
- `PointerButton` **イベント**でボタン状態を `SessionShared.button_mask: u8` に集約（Primary=1, Middle=2, Secondary=4）。現行の per-frame polling (`i.pointer.*_down()`) は廃止。
- 毎フレーム、hover_pos が画像 rect 内 → その fb 座標を送信。押下中（button_mask≠0）で rect 外/ホバー無し → **fb 端へ clamp した座標**を同マスクで送信（ドラッグ継続）。押下無しで rect 外 → 送らない。
- ボタン解放イベントでは clamp 座標 + buttons=0 を必ず送信。
- フォーカス喪失/切断/Close 時は既存の強制解放を使う。
- 画像範囲内でのみ座標送信（バー上は旧実装と同じく送らない）だが、pixel perfect scroll area 内はスクロールオフセットを考慮した rect を使う（現行どおり正しい）。
- リサーチで「TigerVNC が fb 外ドラッグをどう扱うか」が確認でき次第、clamp か continue-last を決める。

### 2-2. Scroll
- 現行の積算+pulse 方式を維持（実測で滑らか）。

### 2-3. Keyboard
- `Event::Key { repeat }` を検出: **長押しはサーバ側の auto-repeat に委ね、`repeat: true` はクライアントでは送出しない**（二重 repeat 防止）。サーバ (Xvnc/XTEST) が autorepeat することを research で確定。
- `Key::F1..F35 → 0xffbe..`、`Key::Space → 0xff20`、`Key::Backtick → 0x0060` ほか句読点/ShiftLeft等/Menu/PrintScreen/Pause/CapsLock/NumLock/ScrollLock を keysym へマップ（MacToLinux では Super/Meta は Control 系へ変換）。
- 修飾キー押下中は `Event::Text` が出ないため、Ctrl 絡みのショートカットも上記マップで届く。
- Caps Lock は「winit/egui から届いた場合は 0xffe5 として転送（best-effort）」。届かない環境があることを docs に明記。**サーバは CapsLock を受けてトグルする**（RFB 仕様上 keysym で Caps_Lock を送れば Xorg/Xvnc が lock を切り替える）。
- Mac Ctrl+Click → right-click（0x0004 mask）への変換を key_preset MacTo* のときのみ実装（標準の macOS 慣例）Identity では変換しない。ユーザー要望次第でトグル化を検討。

### 2-4. その他の不完全実装
- `Event::Text` の改行（\r, \n）は Key Return として扱う（現行: Text 経路で 0x01000000|0x000a となり意味不明な keysym を送っていた）。
- Insert/Cut/Copy/Paste キーのマップ追加。
- Focus loss 時に Pointer mask も 0 へ（release_held の拡張は済みだが PointerButton イベントベースに統一）。

## 3. UI 実装タスク
1. `egui-pop-phosphor` = "0.15.0" を dev（`regular` のみ）として追加。`egui::FontDefinitions` に Phosphor を proportional へ挿入（add_to_fonts）。
2. `install_japanese_fonts` 後に phosphor もロード。`Menlo` monospace family 追加。
3. `build_visuals` を新トークンで全書き換え（light/dark 両方）。hover stroke accent 1px、dividers #262B36。
4. ヘッダー: `G` グリフ丸角スクエア + GlintVNC + version dim + 右側に [＋ 新規] (primary-ghost) [⚙ 設定] (icon ghost)。brand 文字は erase。
5. 左パネル: ヘッダー行（「保存済み接続」+ count pill）。カード再構成（アイコン、接続中 dot、host mono、アクション行= [接続]primary[編集]ghost）。hover で境界 accent、radius 12。
6. 空状態: PLUG 48px + 文言。
7. 右フォーム: セクション見出し（小、dim）+ hairline 区切り 3 群（接続情報/オプション/セキュリティ）。保存ボタン accent 塗り・左下ルール（複数ボタンは gap 12）。
8. 設定: 3 グループ化（表示/リモート接続/このアプリ）+ 行の高さ + divider 線、About にバージョン・LICENSE（GPL風表記はしない、Apache/同等とだけ）。
9. セッション窓の浮動バー: icon+テキスト（10px）、power danger tint、radius 10。
10. ステータスバー: ● N 件接続中 accent dot + 左ステータス。
11. Caption/見出しを句読点無しの plain copy に統一（"WORD — fragment" を作らない）。
12. before/after を screencapture -l<windowID> で比較、削除。
13. DMG、VERIFICATION_LOG、MAINTENANCE（Round 4 行、phosphor/Skill 導入記、frontend-design の由来とライセンス記録）を更新。

## 4. 検証
1. 入力: `cargo test`、mock_rfb.py で bytes 検証（PointerButton マスク、CapsLock keysym、F1、Ctrl+\）→ 既存の integration tests を拡張。
2. 実サーバ: LAN-VNC でクリック/ドラッグ/右クリック/CapsLock/長押しを実機確認（本人手）。GUI 捕捉は理想状態の before/after 比較に使う（remote 画面は映さない）。
3. UI: capture before/after、DMG SHA、docs 更新。

## 5. 実装結果 (2026-11-18)

1. **ドラッグ破綻を根治**: PointerButton を離散イベントとして扱う方式へ全面書き換え。押下中に fb 矩形外（レターボックス/浮動バー/ウィンドウ外を含む）へ出ても **fb 端へ clamp した fb 座標を送り続ける**（TigerVNC と同じ設計）。`map_point`（画像内のみ）と `clamp_map_point` を分離。ボタン解放は必ず clamp 座標で buttons=0 を送る。フォーカス喪失/切断時の強制解放も `button_mask=0` に統一。
2. **右クリック**: 物理右ボタン (Secondary→mask 0x04) は従来どおり正しく送られる。macOS 慣例として **Ctrl+クリックも MacToLinux/MacToWindows プリセットで button3** に変換（Identity は素通し）。`held` 判定・イベント順序も PointerButton に集約。
3. **長押し**: rfbproto (KeyEvent) に従い **クライアント側リピート**（down の繰り返しのみ・up は物理リリース時 exactly-once）。egui の `repeat: true` 検出も使うが、送出は素直な down の繰り返しとして転送。サーバ (Xtigervnc) は `key_repeat=TRUE` として XKB に流すことをソースで確認。
4. **キー拡充**: F1–F35→0xffbe…、Insert 0xff63、Space 0xff20、PrintScreen 0xff61、Pause 0xff13、Menu 0xff67、CapsLock 0xffe5、NumLock 0xff7f、ScrollLock 0xff14。修飾なし印字キーは Text 経路（case を含む生成文字をそのまま送る）に統一し、Ctrl/Cmd/Alt 修飾時のみ Key 経路でもマップ（短縮キーが効くように）。Enter/Tab の改行は Text 側で Return/Tab に変換。
5. **Caps Lock**: macOS は winit が flagsChanged で捨てるためローカル検出は不可能（winit ソースで二重確認）。→ (a) アプリ内 CAPS ボタン（浮動バー、KEYBOARD アイコン）がリモートへ `0xffe5` の down/up を送信、(b) **LED State 疑似符号 (-261) を要求してサーバの lock 状態を受信し、CAPS ボタンを on 表示**（TigerVNC 実機では Xtigervnc が LED state を送る場合のみ状態表示あり、テスト用モック `--led-state` で 0x06 を受信することを実測）。Text 経路は常に正しい case の文字を送るため、大文字入力自体は常に動く。
6. **UI 再設計**: Phosphor regular (egui-pop-phosphor 0.15, MIT/Apache-2.0, egui 0.37 対応系列) 導入、ブランドマーク (G バッジ)、セクション化フォーム（接続情報/オプション/セキュリティ）＋セグメント制御、保存済み接続カード（DESKTOP アイコン + mono host + 接続中 dot + 接続 primary/編集 ghost）、空状態 (PLUG icon)、ステータスバーに接続中 pill (緑ドット)、設定を 表示/リモート接続/このアプリ の3グループ構成に。frontend-design skill の2段階プロセス（トークン→brief照合→実装）に直接適用。before/after はウィンドウ単体キャプチャで確認し削除。
7. **検証**: 30テスト PASS（LED State テスト追加）、LAN 実サーバで接続・リモート描画OK、mock で LED 受信 OK、`scripts/check.sh` PASS、DMG SHA `5d7e7bf…`。浮動バー・CAPS・ドラッグ/長押しの体感確認はユーザー側が必要。

## 6. Skill 導入の記録
- `.opencode/skills/frontend-design/SKILL.md`（Apache-2.0、Anthropic anthropics/skills 197k⭐、SKILL.md 目視確認済み）+ LICENSE.txt。抜粋読了・危険な指示なし（純粋な設計指針）。本プロジェクトの UI はこの skill の二段階プロセスに沿って設計した。導入は `docs/MAINTENANCE.md` §Skills に記録する。
- `ui-ux-pro-max`（MIT）は Python 検索ワークフロー前提で egui では過剰なため導入せず。調査した原則（Priority P1 アクセシビリティ／P2 タッチ／P6 タイポ・カラー／P8 フォーム）のみ参考。
- `web-design-guidelines`（vercel-labs）は Web 専用・実行時 fetch 必須のため導入せず。
