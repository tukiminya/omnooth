# 設計レビューアプリの開発・引き継ぎ

2026-10-27 更新。本体の実装範囲は [ルート README](../README.md) を先に読む。ここはレビューアプリの保守、保存データ、契約の取り出し方を説明する。

同日の次の依頼で本体の開発を開始した。本体の現状は [desktop/README.md](../desktop/README.md) を参照する。最新 Figma の参照は許可されたが、Figma の編集・コメント操作と、このレビューアプリへの製品モックの復元は引き続き行わない。

## 現在の役割

要件・インフラ・DB の ER 図・関数フローを読み、項目単位でコメント・返信・解決・承認を保存するローカルアプリ。製品の画面モックは削除済み。古い `#/contents` は要件ページへフォールバックし、専用画面は存在しない。過去の参照 PNG も配信しない。

Figma はユーザーが手直ししており、エージェントは以後操作しない。古い画面を復元・再生成したり、Figma のコメントを整理し直したりしない。`data/figma-changes-2026-10-23.json` は当時の操作履歴だけで、最新デザインや実装タスクとして扱わない。

最新スコープは unitypackage の明示選択と順次インポート。非 unitypackage の直接追加と、インポート先変更・配置テンプレート・MoveAsset・配置復帰は削除した。ライブラリ保存先の設定と、失敗時のファイル復旧は引き続き対象。

## 起動と環境

Node.js 24、`pnpm@12.3.4`。本セッションの検証環境は Node v24.22.1。依存は `design-review/package.json` と `design-review/pnpm-lock.yaml`、ルートのコマンドは `package.json` を参照する。依存の追加・更新には pnpm を使い、マニフェストを手で書き換えない。

リポジトリのルートで実行する。

```sh
pnpm --dir design-review install
pnpm dev
```

| 用途 | コマンド / 接続先 |
| --- | --- |
| 開発画面 | `http://127.0.0.1:4310/` |
| 開発 API | `http://127.0.0.1:4311/api/catalog` |
| 型チェック + Vite ビルド | `pnpm build` |
| テスト | `pnpm test` |
| ビルド済み画面 + API 配信 | `pnpm start`（4310） |
| 保存レビューの CLI 表示 | `pnpm reviews` |

サーバーはループバックだけで待ち受ける。同一オリジン・ホストと JSON 入力を検証する。Vite の開発プロキシが `/api` を 4311 へ転送する。Node の `node:sqlite` を使い、外部 DB サービスは不要。

過去の制限環境で、pnpm 自身の署名検証がレジストリへの通信失敗で止まったことがある。依存導入済みなら、セキュリティ設定や lockfile を変えずに次の同等コマンドを使える。

```sh
node design-review/server/index.mjs --dev
node --test design-review/server/*.test.mjs
node design-review/node_modules/typescript/bin/tsc -p design-review/tsconfig.json
node design-review/node_modules/vite/bin/vite.js build design-review
node design-review/scripts/reviews.mjs
```

## ファイルの責務

| ファイル | 内容 |
| --- | --- |
| `data/catalog.mjs` | 現行仕様・根拠・関数契約・フローの本文。履歴用の `archivedSpecs` もある |
| `src/types.ts` | レビューアプリの Spec / Catalog / ReviewState。製品 DB 型ではない |
| `src/pages/Workspace.tsx` | ルーティング、一覧、検索、仕様詳細、コメント・承認 UI |
| `src/pages/database-model.ts` | 同じ仕様の fields / FK から ER モデルを作る。分割ビューの配置 |
| `src/pages/DatabaseDiagram.tsx` | ER 図の描画、選択・ズーム・カラム表示 |
| `src/pages/flow-model.ts` | 関数フローと構成図のノード・矢印。カタログの実在 ID と接続する |
| `src/pages/FlowDiagram.tsx` | HTML ノード + SVG 矢印による表示 |
| `src/domain/package-version.ts` | 同期・副作用なしの日付・SHA-256 識別 |
| `src/domain/booth-metadata.ts` | BOOTH 応答の検証・DTO 変換、一意なファイル更新日時の照合 |
| `src/domain/library-content.ts` | unitypackage のみを列挙・選択し、順序付き imports を作る。コピー先の入力や copies はない |
| `server/index.mjs` | Node HTTP API、開発時 Vite 起動、ビルド済み画面の配信 |
| `server/store.mjs` | レビュー SQLite、改訂ハッシュ、入力・承認の検証、自動書き出し |
| `server/fixtures/booth-items.json` | 2商品の実 API 応答を縮小したテスト用フィクスチャ |
| `server/*.test.mjs` | レビューの永続化と改訂、カタログ参照、ER、矢印、参照関数、スコープの回帰テスト |

`ContentPreview.tsx` / `content-examples.ts` / `content-preview.css` と旧参照画像は削除した。モックを復元するためにテストや README へサンプル UI を戻さない。

## レビュー保存と改訂

正本は `data/reviews.sqlite`。`comments` と `decisions` の2テーブルだけを持つ。ER 図にある本体用17テーブルは、この DB には作られていない。

- コメントは targetId / revision / author / body / kind / status / parentId / 時刻を保存する。
- 親コメント・返信の本文を、修正依頼の実施に合わせて書き換えたり消したりしない。返信で対応内容を記録する。
- 未解決の変更依頼がある間は承認できない。スレッド解決と項目承認は別。
- 現行の Spec の JSON から項目ごとの revision を計算する。本文を変えると、古い承認は API / UI では未レビューになる。古い画面からの承認・投稿は 409。
- `decisions` の保存行は過去の revision と承認状態を保つ。CLI の生の行だけで現行仕様が承認済みと判断しない。`/api/catalog` と `/api/reviews` または `/api/export` で現在の revision と表示状態を照合する。
- `archivedSpecs` は削除済み ID の履歴表示用。通常の一覧・検索・図から外れ、承認と新規の変更依頼は 410 で拒否する。既存スレッドの返信・解決はできる。
- 通常の承認は利用者が行う。2026-10-27 の明示的なオートレビュー依頼に限り、委任された対象を Codex（自動レビュー）のメモと根拠付きで承認する。将来の包括的な自動承認は許可されていない。過去の全承認より、後から与えられたユーザーの指示が優先する。

`data/review-snapshot.json` は起動時・変更時の自動書き出し。ブラウザの「レビューを書き出す」からも取得できる。下書きはブラウザの localStorage にあり、送信して保存した本文だけが SQLite に残る。複数人用認証やチャットへの自動送信はない。

レビューを失わずにバックアップするにはサーバーを停止し、`reviews.sqlite` と `review-snapshot.json` をコピーする。稼働中に SQLite 本体だけをコピーしない。WAL を含む稼働中の退避が必要なら SQLite のバックアップ機能を使う。これらのデータは Git 対象外なので、新しいチェックアウトだけではレビューは付いてこない。

## ローカル API

| API | 契約 |
| --- | --- |
| `GET /api/health` | 起動確認 |
| `GET /api/catalog` | 現行 specs / archivedSpecs、根拠、フロー、計算済み revision |
| `GET /api/reviews` | コメントと、現在の revision に合わせた承認表示 |
| `GET /api/export` | 対象名付きの JSON。自動書き出しも更新する |
| `POST /api/comments` | `{targetId, revision, author, body, kind, parentId?}` |
| `PATCH /api/comments/:id` | `{status: "open" または "resolved"}`。親スレッドの返信にも連動 |
| `PATCH /api/decisions/:id` | `{revision, status: "unreviewed" または "approved" または "changes"}` |

kind は `question / change / note`。解決済みスレッドへの返信は親を再オープンする。対応返信の後に解決するなら、この順序を守る。API で他の投稿者の本文を編集する機能はない。

## 仕様を更新するとき

1. 最新のユーザー指示を確認し、必要な項目だけを更新する。削除済み機能を過去のレビューから復活させない。
2. 関数の signature / calls、DB の fields、related、sources、flows を揃える。仕様そのものを消すときは履歴用 ID を `archivedSpecs` に残す。
3. ER 図のビューとフローの配置を更新する。ノードは仕様 ID を参照し、本文を複製しない。
4. 型チェック・影響するテスト・ビルドを実行する。
5. **カタログの変更後は Node サーバーも再起動する。** `catalog.mjs` は起動時に import され、フロントの HMR だけでは本文が更新されない。
6. 自分で作った確認用タブで表示を確認する。ユーザーのレビュー中のタブを勝手に再読み込みしない。Figma の操作は行わない。

## 参照コードと本体実装の境界

参照コードは TypeScript の小さな純粋関数。Node 24 でテストし、将来の Rust ユースケースへ入力・出力ケースを引き継ぐ。`buildContentSelection` は unitypackage の選択と入力不足を確認するだけで、Unity 内部のパス / GUID の解析やインポートを実行しない。

本体の関数契約は、版と明示選択の列から `buildInstallPlan` を作り、選択 ID・順序・unitypackage 内部の Assets マニフェストを planHash に含める。インポート先はパッケージ定義のまま。成功後に `project_packages / project_selections / project_files` を確定する。パス変換・直接配置用の引数、`project_package_paths`、`relocate` は現行設計から削除している。

BOOTH の参照変換は `name` → title、数値 shop.id → externalKey、variation.name=null を許容する。file_size は表示文字列でバイト数ではない。ファイル更新日時は variation ID と元名の完全一致が一意な場合だけ使う。フィクスチャに署名 URL や購入権限は含めない。

レビューアプリの白黒 UI、ER 図、関数フローは引き続き使う。本体用の画面をここで再実装したり、本体 DB をレビューの `server/store.mjs` へ追加したりしない。

## 2026-10-27 のオートレビュー

依頼に従い、スコープ縮小で再レビューになった項目と、日付・SHA-256 統一に伴う更新を確認する。各対象の revision と根拠は `data/auto-review-2026-10-27.json` とレビュー上の Codex（自動レビュー）メモに残す。

完了時点では19項目を委任された自動レビューで承認し、現行75項目すべてが承認済み、未解決スレッドは0件。32件のテスト・型チェック・ビルドが成功した。これは設計の承認であり、本体の動作保証ではない。

版の参照関数は `identifyPackageVersion`。ファイル名由来の版番号、手入力の版名、認識設定は廃止した。通常ファイルは照合できた更新日時、共通カスタムや更新日時不明の原本は初回取り込み日時から UTC 日付を得る。

`server/install-integrity.test.mjs` は複合 FK と一括更新を SQLite の最小スキーマで確認する設計検証で、本体のマイグレーションではない。Unity アダプターの実機動作、OS 認証、展開ライブラリの形式・暗号方式対応は未検証のまま、実装段階で検証する。
