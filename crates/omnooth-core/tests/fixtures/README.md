# Unity export fixture

`unity-2022.3-text.unitypackage` was exported by macOS Unity 2022.3.22f1 on 2026-11-17 using `unity/tests/validate-editor.py`. It contains a self-authored text asset (`omnooth fixture`) and its Unity metadata. It includes the exporter’s `./` paths, PAX timestamps/xattrs and AppleDouble records, so parser tests cover real exporter output. Nothing is extracted to project paths while inspecting it.

SHA-256: `fdddba1bbd0cf39dc5de4e69ef03c79e7c00567f3f01e0de4c2d73ba6ebfe16f`.

`libarchive-sample.rar` は libarchive 公式リポジトリの `libarchive/test/test_read_format_rar.rar.uu` を復号した336バイトのテスト用アーカイブ。出典: https://github.com/libarchive/libarchive/blob/master/libarchive/test/test_read_format_rar.rar.uu 。ライセンスは同梱 `libarchive-COPYING.txt` を参照。RAR のストリーム展開を検証し、Unity に適用しない。

`test_read_format_rar5_*.rar` も同じ libarchive/test の対応する `.uu` ファイルから取得した。RAR5 の正常展開、暗号化されたヘッダーのパスワード待ちと再試行に使う。テスト用の既知パスワードは `password`。通常ヘッダーの暗号化 fixture はファイルごとに異なるパスワードを含む。複数の異なるパスワードを1回の展開で要求する UI は実装しない。
