# GlintVNC ラウンド7 — 残像とラグ: サーバの実態と対策

対象問題（ユーザー再報告, 2026-11-19）:
「windowを移動させた時残像がずっと残のと、ラグい」

## 1. サーバの実態（2026-11-19, SSH `ps -ef` で確定）

```
user 87838 1 0 11月16 ? 00:12:18  x11vnc -forever -shared -usepw -bg -noscrollcopyrect
```

- **x11vnc 0.9.x**（LibVNCServer 系 / 物理 `:0` ディスプレイを共有）。TigerVNC / Xtigervnc ではない（長年誤認していた）。
- `-usepw`: VNCパスワード認証。`-forever -shared`: 常時起動・複数クライアント同時接続可。`-bg`: バックグラウンド。
- `-noscrollcopyrect`: **スクロールの CopyRect 最適化をユーザーが意図的に無効化**。（x11vnc の `-scrollcopyrect` は RECORD X 拡張で `XConfigureWindow`/`XCopyArea` を snoop してスクロールを CopyRect 化するヒューリスティック。有効だと「誤爆」やティアリングが起きるため意図的に外しうるフラグ。）
- 窓のドラッグ移動は `-wirecopyrect`（既定 `always`、有効）で検出され CopyRect になるはず… **しかし実測では一度も CopyRect を観測できない**（`copyrect=0` が連続）。XDamage 未使用または CopyRect 判定が効かない環境の可能性が高い。

### x11vnc の構造（なぜラグいか）
x11vnc は **XShmGetImage で fb をポーリング**する方式（デフォルト `-wait 20` / `-defer 20`、つまり最大 ~50Hz だが実効はさらに低い）。
- fb 読み取り自体が重い（GPU→CPU コピー）。大画面を一度に読むと 0.5–1s かかる旨が README にも記載あり。
- `-wait_ui 2.0`: マウス/キーボード操作中は poll 間隔を半分にする自動高速化（ドラッグ中は効いているはず）。
- `-setdefer 1`: wait_ui で wait が下がると defer も同じ ms に。
- それでも fb 読み取り・比較・zlib 圧縮はサーバ CPU を食う。**リモートは小型 OptiPlex で H.264 も ncache も使えない**。

## 2. 残像の仮説（確定事項+妥当な推論）
x11vnc の増分更新は XDamage で検出した領域のみを送る設計だが、比較/合併ロジックが変化を取りこぼす経路がある（特に大きく動く窓＋露出領域の合成描画）。クライアントが**増分更新のみ**を要求する限り、取りこぼした正しい画素は二度と届かない → **残像が永遠に残る**。
- これはクライアント側が満額描画していても防げない（欠落はサーバの責任）。
- クライアントの防御策は「非増分リフレッシュをいつでも投げられる」こと。これが Refresh 機能（後述）の根拠。

## 3. ラグの構図
1. ドラッグ中は大きな矩形が毎ポーリング再エンコードされる
2. ZRLE の zlib 圧縮はサーバ CPU に乗るため、**CompressLevel が高いとサーバがボトルネック**になる
3. xfwm4 コンポジター OFF（済）でも x11vnc の再エンコードコストは残る

## 4. 対応（今回実装, ラウンド7 クライアント側）
- **Refresh (再描画) 機能を追加**: 浮動バーに `↻ 再描画` ボタン + **⌘R**。非増分 `FramebufferUpdateRequest` を送り、サーバに全画面を再送させる → 残像は即消去。クライアント側は `Event::Frame{rects=[full]}` を既存 dirty-rect パイプラインで適用するだけ（新規コードは最小）。
- **CompressLevel 1（-255）広告**: `SetEncodings` に `-255` を追加（x11vnc=LibVNCServer はクライアントの CompressLevel 指定を尊重）。サーバのエンコード CPU 負荷を下げる。
- **統合テスト `refresh_triggers_full_framebuffer_request`**: モックが非増分リクエストの到着をバイト単位で検証。

## 5. サーバ側チューニング（ユーザー判断・私は live なデスクトップに触れない）
以下はユーザーが再起動する x11vnc のコマンドラインで:
- `-wait 10 -defer 10`（応答性向上・CPU 負荷増）
- `-xrefresh 120`（2分毎の強制全画面再送 → 残像が自動回復。トラフィック増）
- `-wirecopyrect always`（既定 ON のはず）+ XDamage が有効か要確認（`xdamage` があるか）
- `-noscrollcopyrect` は据え置きでよい（窓移動には関係しない）

## 6. 検証結果
| 項目 | 結果 |
|---|---|
| `scripts/check.sh`（fmt/clippy/test) | PASS (45 tests) |
| 45 tests | PASS（新規 `refresh_triggers_full_framebuffer_request` を含む） |
| Refresh のワイヤ動作 | 統合テストで byte 検証済み |
| LAN probe | PASS |
| 残像消去の実体験 | ユーザーの実機ドラッグ後の手動テスト待ち |
| DMG | SHA256 de8e27ecb9ecdad2764864f176af38c5ef70eea058a9e4c5bdad466c3b52f158 |
