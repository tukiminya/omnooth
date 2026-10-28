# 初期実装の検証記録

2026-10-27。今回の開発単位は [実装計画](implementation-plan.md)、機能の制約と起動方法は [本体 README](../desktop/README.md) を参照する。

## 2026-10-28：アイコンと配布ビルド

- 指定アプリアイコンの同梱 SVG から Tauri CLI で PNG / ICO / ICNS を生成し、生成した PNG を目視確認した。macOS の `.app` 内の ICNS とソース側の ICNS の SHA-256 が一致した。favicon も同じ SVG を使う。
- 汎用アイコンを `lucide-react` に移し、以前の汎用 SVG を削除した。残る画面内の画像要素は独自ワードマークだけ。1280 / 1440 / 1920px の画面キャプチャで欠落・配置崩れがないことを確認し、既存 Playwright 11件が成功した。
- TypeScript / Vite、`cargo fmt --all --check`、`cargo test --offline --workspace` が成功。Rust は core 26件、本体3件、Unity 3件。OS の既定ハンドラーを変更する1件は通常どおり実行対象外とした。
- CI 用設定をマージしたローカルの macOS デバッグ `.app` ビルドが成功した。`codesign --verify --deep --strict` が成功し、Info.plist の ID は `dev.tuki.omnooth`。今回は ad-hoc 署名で、notarization は行っていない。
- GitHub Actions に Windows x64 NSIS、macOS Apple Silicon / Intel DMG、Linux x64 DEB / AppImage と、全件成功後の Release 下書き作成を追加した。`actionlint` と Node の成果物検証テスト4件が成功。Tauri Action が追加で出力する macOS `.app` を除外し、5インストーラーを揃えてから添付する。欠落・拡張子不一致・未知の成果物を拒否するケースも確認した。
- Windows でディレクトリを `std::fs::File` で開いて同期しないよう Unity アダプターを修正した。ファイル本体の同期は各 OS で行い、ディレクトリの同期は Unix で行う。

GitHub Actions はまだ実行していない。Windows / Linux / macOS Intel のビルド成功や、各 OS でのインストール・起動を確認したという意味ではない。今回のローカルビルドは `.app` までで、DMG 自体の生成検証も CI 実行後に確認する。[配布手順と制約](releases.md) に公開までの流れを記載した。保護対象3ファイルの SHA-256 は末尾の記録から変わっていない。

## 2026-10-27：ID・受信設定・画面修正

- `dev.tuki.omnooth` への変更を生成した Info.plist で確認し、CFBundleURLTypes に booth-library-manager が含まれることを確認した。
- 旧IDの DB を残したまま、新IDの DB へ Eku の登録と以前のライブラリ保存先が引き継がれたことを読み取り確認した。通常のテストでも、稼働中の WAL を含むバックアップと、新IDの既存 DB を上書きしないケースを確認した。
- Rust は core 26件、本体3件、Unity 3件の計32件が成功。別途、明示実行する macOS の登録テスト1件で、生成した .app を Launch Services に登録し、booth-library-manager の既定ハンドラーが dev.tuki.omnooth になったことを OS API で読み戻した。
- ブラウザ11件が成功。一般／履歴の反復切り替え、アクティブボタンが1件であること、未保存値の保持・キャンセル、Deep Link の設定操作、起動時・実行中の受信通知、マニュアルの指定URL、連携ナビゲーションの削除を含む。ページの未処理エラーも検査した。
- TypeScript / Vite と macOS の開発用 .app ビルドが成功。操作ツールが旧 application ID を参照し続けたため、新IDのネイティブ UI 操作による OS 経由の受信確認は未完了。OS 登録の確認と、受信処理・通知の自動テストは区別する。

macOS の OS 登録テストは実際の既定ハンドラーを変更するため、通常の cargo test では ignore している。明示的な検証時のみ、OMNOOTH_REGISTRATION_TEST_BUNDLE に対象 .app を指定して実行する。以下の表は初回の開発単位の記録。

## 自動検証

| 対象 | 結果 |
| --- | --- |
| Rust `cargo test --offline --workspace` | core 18件、Unity transport 3件、すべて成功 |
| Rust `cargo fmt --all --check` / `cargo check --offline -p omnooth-desktop` | 成功 |
| 画面 `pnpm --dir desktop test` | Playwright 7件成功。ブラウザ確認用データは tests 内だけ |
| 本体 TypeScript / Vite | 成功 |
| Tauri CLI `build --debug --bundles app` | macOS の開発用 .app を生成。署名・配布の検証ではない |
| レビューアプリ `node --test design-review/server/*.test.mjs` | 既存32件すべて成功 |
| レビューアプリ TypeScript / Vite | 成功。出力は design-review/dist |

画面テストは、初期の未選択状態、unitypackage の未選択・明示順序・版変更での選択解除、設定のキャンセルと保存、多言語切替、複数取り込みの一部失敗、1280 / 1440 / 1920px での表示と SVG の読み込みを確認した。初期化時の繰り返し言語変更と、React StrictMode における dialog の即時閉鎖を検出・修正した。

本体の初回起動を、一時データフォルダーでの SQLite 作成と、生成した .app の通常起動で確認した。通常起動では実際の appData の保存先が GUI に表示され、ファイル選択をキャンセルすると原本が追加されないことを確認した。テスト用の商品・プロジェクトを通常起動の DB に投入していない。

Unity Editor 拡張はインストール済み Unity 2022.3.22f1 の Mono コンパイラーと Editor / Engine / JSON API アセンブリに対してコンパイルできた。Editor の実起動による検証は LicensingClient の IPC 接続タイムアウトで停止したため、実 ImportPackage・コールバック・ドメインリロード中の挙動は未検証。本体の適用ボタンは無効のまま。

## フォント

指定された LINE Seed JP / Gen Interface JP と JetBrains Mono をローカル配信する。元の TTF と全字形 WOFF2 のサイズは `desktop/public/fonts/sizes.json`、画面文字版のサイズは `subsets.json` に保存した。全字形版を削除せず、`unicode-range` で必要に応じて読み込む。

| 比較 | 取り込み前 | 取り込み後 |
| --- | ---: | ---: |
| LINE Seed JP Regular | 3,673,280 B TTF | 1,512,600 B WOFF2 |
| LINE Seed JP Bold | 3,786,144 B TTF | 1,552,088 B WOFF2 |
| Gen Interface JP Regular | 6,161,392 B TTF | 2,662,744 B WOFF2 |
| Gen Interface JP Medium | 6,159,728 B TTF | 2,684,212 B WOFF2 |
| 初期プロジェクト画面のフォント転送量 | 6,899,944 B（全字形） | 201,144 B（画面文字版） |

最後の行は Playwright の新しい Chromium コンテキスト、日本語、1440×900、ローカル Vite サーバー、HTTP キャッシュのない状態で PerformanceResourceTiming を取得した単回の測定。転送量には応答のオーバーヘッドを含む。画面文字版は3フォントを読み、所要時間はそれぞれ約14〜16ms、font ready はページ開始から約131msだった。ループバックの測定であり、OS キャッシュを排除した反復ベンチマーク、CDN との速度比較、ネイティブ WebView の起動時間の比較は未実施。ローカル配置だけによる一般的な速度向上を断定しない。

## 保護対象

作業途中と完了時の SHA-256 が一致している。

- `dist/omnooth`: `c742c6671d8f2806e47adb09cd9a09c4a9352f62f5aad0b586433a1939c09d63`
- `design-review/data/reviews.sqlite`: `ade3e04e107545ed37607f8525551a3153bd84682d9244ab8cab4f2e28efc1fd`
- `design-review/data/review-snapshot.json`: `0a1c56005f14355bafcd1d72386c6049cd5f243c3896b534db7dfbda6e54b14d`

Figma は参照だけで編集・コメント操作をしていない。カタログと承認は更新していない。本体の初期スキーマを設計レビュー DB に追加していない。
