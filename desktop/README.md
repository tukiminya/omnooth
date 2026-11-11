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

`main` への push（マージを含む）では GitHub Actions が Windows x64、macOS Apple Silicon / Intel、Linux x64 の配布用インストーラーを生成し、全ビルドの成功後に GitHub Release の下書きへ添付する。手動実行も可能。[配布手順](../docs/releases.md) に対象形式、再実行、署名の制約を記載した。

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

起動時と実行中の `booth-library-manager://item-import` をバックエンドで受信・検証し、商品 ID と元ファイル名などの安全な受付情報を表示する。署名 URL と注文 ID は画面へ渡さない。受信後のダウンロード・永続 FIFO・ライブラリ登録は「BOOTH の取り込み」に記載する。

## BOOTH の取り込み

受信したリンクは永続 FIFO へ入り、ワーカーが1件ずつダウンロード、公開メタデータ取得、原本の SHA-256 確認とライブラリ登録を行う。商品メタデータが取得できない場合は警告として記録し、元ファイル名と取り込み日時を使って登録を続ける。版更新日時は variation ID と元ファイル名が一意に一致した BOOTH 公開メタデータだけから取得する。

同じ `sourceIdentity` の受信は既存ジョブにまとめる。待機中のジョブは新しい受付 receipt と URL を使い、受付時に保存した設定は保持する。ダウンロード中の重複は別ジョブを作らず、現在の処理と表示は最初に受け取った receipt のまま進める。失敗・期限切れ時に新しい URL が届いていれば、同じ job ID を新しい receipt で再待機し、次の試行に使う。それ以外は新しい Deep Link を待つ。成功済み source の再通知は保存済み結果を返し、再ダウンロードしない。

各ジョブの `libraryPath` と `extractArchives` は Deep Link を受け取った時点で固定する。その後に一般設定を変更しても、待機中のジョブには反映しない。再起動時、保存済みの `queued` / `downloading` ジョブは URL が失われるため `awaitingUrl` になり、新しい Deep Link で同じジョブ ID と初回設定を使って再開する。期限内の自動再開は未実装。

署名付き `dlurl` と `order_id` はプロセス内の `ParsedItemImport` にのみ保持し、queue JSON へ書き込まない。JSON は `id`、安全な `receipt`、`extractArchives`、`libraryPath`、`status`、`errorCode`、`localItemId` のみを含む。メモリー上限は2000件。`appData/logs/` のセッション JSONL は各256 KiBまでを4セグメントで循環し、最大8セッション分を保持する。OS 資格情報による暗号化は未実装であり、期限前 URL の自動復旧も行わない。

画面下部の「ログ」は「マニュアル」の直上にある。現在のセッションを表示し、ダイアログを開いている間は1秒ごとに更新する。情報・警告・エラーのレベル絞り込みと手動更新ができる。ロガーは URL と、`dlurl`、`order_id`、password/token、authorization 等の既知の機密キーに続く値を伏せる。

BOOTH 実ファイルの進行状況と未完了の検証項目は [BOOTH 取り込み検証記録](../docs/booth-import-validation.md) を参照する。

マニュアルは既定ブラウザで `https://omnooth.tuki.dev/docs` を開く。連携専用画面は削除した。

## この段階で動く範囲

- Assets と ProjectSettings/ProjectVersion.txt を持つ既存 Unity プロジェクトの検証・登録。同一実体を重複登録しない。
- ローカル配布原本の SHA-256 保存と再取り込み、日付＋ハッシュの版表示。同じ原本の初回日付を保持する。
- ZIP の明示的な展開。パストラバーサル、シンボリックリンク、特殊ファイル、重複パス、ファイルとディレクトリの衝突を拒否し、件数と実際の展開バイト数を制限する。
- 未展開 ZIP の収録パス・種類・宣言サイズの表示と検索。中身を解凍せずに unitypackage を選択できる。暗号化メンバーは一覧に表示するが選択不可。読み込み失敗時は再試行できる。
- unitypackage の一覧と明示選択・順序変更。保存済みの原本と版の選択を分け、初期状態では何も選択しない。
- ライブラリ保存先、言語、取り込み時の展開設定の保存。保存先変更は以後の取り込みに適用し、以前の原本の絶対パスも保持する。既存の原本は移動しない。
- 日本語・英語・韓国語の画面。初回の本体の保存言語は現在 ja。ブラウザ確認では地域コードを正規化する。OS ロケールを本体の初回設定へ渡す処理は後続。
- 複数のローカルファイルの取り込みで、一部が失敗しても成功分と失敗ファイルを分けて表示する。
- BOOTH Deep Link の永続 FIFO、ダウンロード、公開メタデータ取得、ローカルライブラリ登録。受信時のライブラリ保存先と展開設定をジョブに保存し、成功済み source を再ダウンロードしない。
- 起動・command・取り込みエラーを記録する永続活動ログ。署名 URL などは redaction し、UI は現在のセッションのログを表示する。

ローカルファイル取り込みでは元ファイル名の完全一致を同一パッケージの仮の識別に使う。BOOTH 取り込みでは商品 ID と variation・配信元のパスから作った `sourceIdentity` を使う。BOOTH 公開メタデータの更新日時は、variation ID と元ファイル名の一意な一致を確認した場合だけ使い、取得できない場合は取り込み日の UTC 日付にする。どちらもファイル名から版番号を抽出しない。共通カスタム・依存・適用台帳用の本体17テーブルをすべて実装した段階ではない。

7z / RAR / tar 系は原本の保存までで、展開を依頼した場合は未対応であることを表示する。暗号化 ZIP のパスワード入力・再開は未実装。unitypackage は原本のまま保持し、内部 Assets / GUID の解析・検証は後続。

## Unity への適用と後続作業

`crates/omnooth-unity/` と `unity/com.omnooth.editor/` に独立したアダプターを追加した。Editor API に対する C# コンパイルと Rust の通信テストを確認した。実機の ImportPackage の完了確認は LicensingClient の IPC 接続が利用できず未完了。導入と制約は [アダプターの README](../unity/com.omnooth.editor/README.md) を参照する。

本体の「適用」はまだ有効にしていない。planHash、内部 Assets / GUID の競合検査、依存解決、所有台帳、バックアップ、実ファイルとの照合、DB 確定・復旧が揃ってから接続する。単に Editor の完了コールバックを受けたことを本体のインストール成功とは扱わない。

2026-11-06 に ZIP 用の一時取り出しとアダプター経路を追加した。原本の SHA-256 を照合し、明示選択した unitypackage のみを `appData/unity-staging/<jobId>/` へ取り出す。`start_zip_import` が Editor へ要求を送り、`poll_zip_import` が同じ要求の全件完了を確認すると一時領域を削除する。待機・失敗・キャンセル・不一致では保持し、再起動後も同じ job ID から確認できる。これは独立アダプターの経路であり、本体の適用 UI・計画・台帳への接続や Unity 実機での確認は未完了。[ZIP 処理の詳細と検証](../docs/zip-import.md) を参照する。

共通カスタム、検索演算子、OS 認証、外部拡張の標準入出力は後続。過去の版は現在すべて保持し、保持設定の保存値を使う安全な整理処理も後続。BOOTH の署名 URL を OS 資格情報で暗号化して保管する処理、再起動後に期限内の URL を自動再開する処理は未実装。今回の実装計画は [実装計画](../docs/implementation-plan.md) に記録した。

## Figma とフォント

最新の [Figma](https://www.figma.com/design/aKMF6XjaOU2Hz7Bj9GxPXY/omnooth?m=dev) を参照した。書き込み・コメント操作は行わない。プロジェクト画面 36:3554、商品画面 22:466、設定 36:4080、パッケージ画面 39:5574 の構造とアセットを参照し、最新仕様に合わせて対象外の操作を除いた。全画面の実装完了や全状態のピクセル一致を意味しない。

汎用アイコンは `lucide-react` の名前付き import を `src/Icon.tsx` でまとめて使う。Figma 由来の汎用 SVG は削除し、独自のワードマークだけを `public/assets/` に残した。指定されたアプリアイコン（Figma `127:1137`）の原本は `public/brand/app-icon.svg`、Tauri が使う生成済み PNG / ICO / ICNS は `src-tauri/icons/`。Web の favicon も同じ原本を使う。生成物は同梱済みなので、CI で Figma に接続しない。

macOS 用の `icon.icns` は、原本の白い背景を角丸に切り抜き、1024px の透明キャンバスに 824px のタイルとして配置する。Dock で四角い白背景が端まで表示されるのを防ぎ、周囲のアイコンと表示サイズを揃える。原本や他 OS 用アイコンを更新した後は、依存導入済みの環境で `node desktop/scripts/generate-macos-icon.mjs` を実行して ICNS を再生成する。開発時と `.app` / DMG の両方がこの ICNS を使う。

フォントは `public/fonts/` に同梱し、実行時の CDN 接続を不要にした。[LINE Seed JP](https://github.com/line/seed)、[Gen Interface JP](https://github.com/yamatoiizuka/gen-interface-jp)、[JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) のライセンスを `public/licenses/` に保存している。

LINE / Gen はこの環境に配置済みの TTF から WOFF2 に変換し、容量が約56〜59%減った。`sizes.json` に実測値を保存した。画面文字の軽量版と、任意の商品名のための全字形版を `unicode-range` で使い分ける。画面文言を変えた場合は `fonttools[woff]` を用意して `python3 desktop/scripts/subset-fonts.py` で再生成できる。全字形版を残すので未収録の商品名も表示対象から失われない。測定結果と制約は [検証記録](../docs/implementation-validation.md) に残す。
