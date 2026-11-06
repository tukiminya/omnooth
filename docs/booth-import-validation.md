# BOOTH 取り込み検証記録

2026-10-28。実装済みの処理、Firefox で実操作した結果、ネイティブ UI の確認制約を分けて記録する。

## 接続済みの処理

1. `booth-library-manager://item-import` を起動時・実行中に受信し、入力を検証する。
2. 安全な receipt から `sourceIdentity` を照合し、永続 FIFO へ受付時の `libraryPath` と `extractArchives` を保存する。
3. 一件ずつ、メモリー内の署名 URL で BOOTH 原本をダウンロードする。
4. 公開メタデータを取得し、取得できなければ警告を残してファイル名と取り込み日時で続行する。
5. 原本の SHA-256 を照合し、ライブラリへ登録する。
6. 成功済み source の再受信には保存済みの結果を返す。失敗・期限切れは同じ job ID で新しいリンクを待つ。

Queue JSON に署名 URL と `order_id` を保存しない。再起動で未完了ジョブを自動実行せず `awaitingUrl` に変える。期限内の自動再開と OS 資格情報を使った URL 暗号化は未実装。

## 現在の手動確認

- BOOTH 商品 ID `7328764` から `Eku_PC_v1_3_0.zip` のリンクを受信した。
- Firefox の「その他のDL方法」から「BOOTH Library ManagerでDL」を実際に押した。初回調査では、既存コードが受信通知のみでダウンロード・登録に進まないことを確認した。最新ビルドへの更新・再起動と、OS の受信先の再登録を行った。
- 実ファイル `Eku_PC_v1_3_0.zip` の **1,193,733,811 bytes** をダウンロードし、商品名 `【オリジナル3Dモデル】エク - Eku #Eku3D` と BOOTH 商品 ID の関連、原本、版の登録を SQLite とログで確認した。ジョブは `succeeded`、エラーは null。
- 保存原本を別途 SHA-256 で読み直し、`57086b9b1519e46f5a24eecd69031cd322d98f831d1578aa55f3e051988f8f6f` と一致した。版の日付は公開情報に基づく `2026-10-18`。現在の `extractArchives=false` に従って `stored` とし、ZIP を自動展開していない。Unity へは適用していない。
- `library.import` の成功記録後に画面の `getSnapshot` が成功した。画面側のライブラリ再取得が動いたことをログで確認した。
- アプリを通常終了し、最新ビルドへ更新後に Firefox から同じリンクを再度開いた。起動・キュー復元・新リンク受信・登録済み判定を新セッションのログで確認した。このセッションの `download.start` は0件、エクの版は引き続き1件だった。
- Firefox の BOOTH ページには「アプリの起動に失敗しました。ご案内ページに移動しますか？」も表示された。実際の受信・登録済み判定と併存することを確認し、案内への移動はキャンセルした。このページ側の案内が出なくなることは確認できていない。
- ネイティブ UI 操作ツールは `Invalid app: dev.tuki.omnooth` や旧IDに対する `Running application not found` を返し、名前・パス・新旧ID・ツール再初期化でも接続できなかった。したがって omnooth の画面をこのツールで直接クリックしての最終確認は未完了。Firefox の操作、実アプリのログ、保存データと原本を照合した結果と、画面の自動テストを区別する。
- Figma の操作は行っていない。
- 保護対象の `dist/omnooth`、`design-review/data/reviews.sqlite`、`design-review/data/review-snapshot.json` の SHA-256 は [既存の検証記録](implementation-validation.md#保護対象) と一致した。レビュー・承認履歴は変更していない。

## 最終統合検証

- Rust `cargo test --offline --workspace`: core 39件、本体15件、Unity 3件、計57件成功。OS の受信先を変更する1件は通常実行では除外し、別途、ビルド済みバンドルを指定した明示テストで成功した。
- `cargo fmt --all --check` と `git diff --check`: 成功。
- TypeScript / Vite: 成功。macOS デバッグ `.app` のビルドと `codesign --verify --deep --strict` も成功した。
- Playwright 19件すべて成功。ログの表示・更新・フィルター・失敗・HTMLとして解釈しない表示、取り込み完了時の再取得・選択、再試行、起動URLと通常起動の遷移を確認した。
- ネイティブ UI の直接確認は上記のツール接続制約で未完了。他 OS の実行と GitHub Actions は今回実行していない。

ログ画面はマニュアル直上から開き、現在の起動セッションを1秒ごとに更新する。ダウンロード進捗は5秒間隔で受信バイト数を記録し、タイムアウト・接続・body・HTTP status を URL を含めず区別する。現在の実機取り込みはこの進捗ログ追加前のビルドで完了し、追加後のコールバックは自動テストで確認した。署名 URL の暗号化保管と再起動後の自動再開は後続であり、今回の実機成功を全設計・全ファイル形式の検証済みとは扱わない。
