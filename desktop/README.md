# omnooth 本体の開発

2026-10-27 に実装を開始した。設計レビューアプリは `design-review/` に独立して残している。本体は Tauri 2 / React / TypeScript、共通の application service は `crates/omnooth-core/`。

## 起動

Node.js 24、pnpm 12、Rust と OS ごとの Tauri 開発環境が必要。

```sh
pnpm --dir desktop install --frozen-lockfile
pnpm app:dev
```

`pnpm app:web` は `http://127.0.0.1:1420` に空の画面を表示する。ファイル選択と永続化にはデスクトップアプリを使う。通常の起動でサンプル商品・プロジェクトを作らない。

```sh
pnpm app:check                  # TypeScript + フロントビルド
pnpm app:test                   # Rust の全 crate のテスト
pnpm --dir desktop test         # ブラウザ操作テスト
pnpm --dir desktop exec playwright install chromium
pnpm app:build                 # 本体のリリース実行ファイル。bundle は作らない
```

フロントの出力は `desktop/dist/`、Rust の出力は `target/`。既存の `dist/omnooth` を上書きしない。Tauri の JS API / Rust の minor バージョンを揃える。現在は API 2.11 系と dialog 2.7 系を固定している。

デバッグビルドのみ `OMNOOTH_DEV_DATA_DIR` で検証用 DB と保存庫を一時フォルダーへ分離できる。通常は Tauri の appData に `omnooth.sqlite` と `Library/` を作る。

application ID は `dev.tuki.omnooth`。旧 `jp.tukiminya.omnooth` の DB がある場合、新ID側にまだ DB がなければ SQLite のオンラインバックアップで引き継ぐ。旧 DB と原本は削除・移動せず、元の絶対パスも維持する。新ID側に DB がある場合は上書きしない。

## BOOTH リンクの受信設定

設定の「一般」から「BOOTH リンクの受信先に設定」を実行すると、`booth-library-manager` の既定の受信先を omnooth に切り替える。すでに別のアプリが登録されていても設定し直し、OS から読み戻して登録を確認する。この操作は一般設定の「保存する」を待たずに反映される。

macOS は `.app` から設定する。`pnpm app:dev` の未バンドル実行では `BUNDLE_REQUIRED` を返すため、次の開発用ビルドを起動する。

```sh
pnpm --dir desktop tauri build --debug --bundles app --config '{"bundle":{"active":true}}'
```

生成先は `target/debug/bundle/macos/omnooth.app`。macOS は Launch Services でアプリを登録して既定のハンドラーを選び、Windows / Linux は Tauri の Deep Link プラグインで既定の登録を更新する。

起動時と実行中の `booth-library-manager://item-import` をバックエンドで受信・検証する。商品 ID と元ファイル名などの受付情報だけを表示し、署名 URL と注文 ID を画面へ渡さない。今回の追加は受信通知までで、自動ダウンロード・永続 FIFO は未接続。受信情報は本体のメモリー上に保持し、再起動後に残るキューとしては扱わない。

マニュアルは既定ブラウザで `https://omnooth.tuki.dev/docs` を開く。連携専用画面は削除した。

## この段階で動く範囲

- Assets と ProjectSettings/ProjectVersion.txt を持つ既存 Unity プロジェクトの検証・登録。同一実体を重複登録しない。
- ローカル配布原本の SHA-256 保存と再取り込み、日付＋ハッシュの版表示。同じ原本の初回日付を保持する。
- ZIP の明示的な展開。パストラバーサル、シンボリックリンク、特殊ファイル、重複パス、ファイルとディレクトリの衝突を拒否し、件数と実際の展開バイト数を制限する。
- unitypackage の一覧と明示選択・順序変更。保存済みの原本と版の選択を分け、初期状態では何も選択しない。
- ライブラリ保存先、言語、取り込み時の展開設定の保存。保存先変更は以後の取り込みに適用し、以前の原本の絶対パスも保持する。既存の原本は移動しない。
- 日本語・英語・韓国語の画面。初回の本体の保存言語は現在 ja。ブラウザ確認では地域コードを正規化する。OS ロケールを本体の初回設定へ渡す処理は後続。
- 複数のローカルファイルの取り込みで、一部が失敗しても成功分と失敗ファイルを分けて表示する。

ローカル取り込みでは元ファイル名の完全一致を同一パッケージの仮の識別に使う。ファイル名から版名や版番号を抽出しない。BOOTH 商品・variation による識別、公開 API の更新日時はまだ接続しておらず、版の日付は初回取り込みの UTC 日付。共通カスタム・依存・適用台帳用の本体17テーブルをすべて実装した段階ではない。

7z / RAR / tar 系は原本の保存までで、展開を依頼した場合は未対応であることを表示する。暗号化 ZIP のパスワード入力・再開は未実装。unitypackage は原本のまま保持し、内部 Assets / GUID の解析・検証は後続。

## Unity への適用と後続作業

`crates/omnooth-unity/` と `unity/com.omnooth.editor/` に独立したアダプターを追加した。Editor API に対する C# コンパイルと Rust の通信テストを確認した。実機の ImportPackage の完了確認は LicensingClient の IPC 接続が利用できず未完了。導入と制約は [アダプターの README](../unity/com.omnooth.editor/README.md) を参照する。

本体の「適用」はまだ有効にしていない。planHash、内部 Assets / GUID の競合検査、依存解決、所有台帳、バックアップ、実ファイルとの照合、DB 確定・復旧が揃ってから接続する。単に Editor の完了コールバックを受けたことを本体のインストール成功とは扱わない。

Deep Link からのダウンロード、永続 FIFO、BOOTH メタデータ、共通カスタム、検索演算子、OS 認証、外部拡張の標準入出力は後続。過去の版は現在すべて保持し、保持設定の保存値を使う安全な整理処理は後続。今回の実装計画は [実装計画](../docs/implementation-plan.md) に記録した。

## Figma とフォント

最新の [Figma](https://www.figma.com/design/aKMF6XjaOU2Hz7Bj9GxPXY/omnooth?m=dev) を参照した。書き込み・コメント操作は行わない。プロジェクト画面 36:3554、商品画面 22:466、設定 36:4080、パッケージ画面 39:5574 の構造とアセットを参照し、最新仕様に合わせて対象外の操作を除いた。全画面の実装完了や全状態のピクセル一致を意味しない。

フォントは `public/fonts/` に同梱し、実行時の CDN 接続を不要にした。[LINE Seed JP](https://github.com/line/seed)、[Gen Interface JP](https://github.com/yamatoiizuka/gen-interface-jp)、[JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) のライセンスを `public/licenses/` に保存している。

LINE / Gen はこの環境に配置済みの TTF から WOFF2 に変換し、容量が約56〜59%減った。`sizes.json` に実測値を保存した。画面文字の軽量版と、任意の商品名のための全字形版を `unicode-range` で使い分ける。画面文言を変えた場合は `fonttools[woff]` を用意して `python3 desktop/scripts/subset-fonts.py` で再生成できる。全字形版を残すので未収録の商品名も表示対象から失われない。測定結果と制約は [検証記録](../docs/implementation-validation.md) に残す。
