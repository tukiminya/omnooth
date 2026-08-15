# omnooth

> [!CAUTION]
> omnooth は実験的な開発期間中であり、正常に動作しないことがほとんどです。
>
> **実使用は避けてください。**

omnooth（オムノース）は、BOOTHのライブラリからダウンロードしたファイルを整理するためのマルチプラットフォームCLIです。

## 対応環境

- Windows、macOS、またはXDG準拠のLinuxデスクトップ

## はじめる

```sh
go build -o omnooth .
./omnooth scheme install
```

これで、BOOTHから開かれた `booth-library-manager://` URLをomnoothが受け取れるようになります。管理者権限は不要です。

登録状態と直近の取込結果は、次のコマンドで確認できます。

```sh
./omnooth scheme status
```

## インポート

通常はBOOTHから自動的に開始されます。Custom URLを直接処理する場合は、次のように実行します。

```sh
./omnooth import 'booth-library-manager://item-import?dlurl=...&downloadable_filename=...&item_id=...&order_id=...&variation_id=...'
```

ギフト商品では `order_id` が省略されることがありますが、そのまま取り込めます。

取込結果は次の構造で保存されます。

```text
~/omnooth/items/
  {SHOP_ID}_{SHOP_NAME}/
    {ITEM_ID}_{ITEM_NAME}/
      {downloadable_filename}/
        {ダウンロードしたファイル、または展開内容}
```

## 対応書庫

- ZIP
- TARおよび圧縮TAR（`.tar.gz`、`.tar.bz2`、`.tar.xz`、`.tar.zst`など）
- 7z
- RAR

書庫は展開して保存します。通常ファイルはそのまま保存します。パスワードで暗号化された書庫には対応していません。

## URLスキームの管理

```sh
./omnooth scheme install
./omnooth scheme status
./omnooth scheme uninstall
```

バイナリを更新した場合は、更新後のバイナリから `scheme install` を再実行してください。`scheme uninstall` を実行しても、取込済みの商品は削除されません。

omnoothは、HTTPSの `booth.pm` またはそのサブドメインから提供されるダウンロードだけを受け付けます。

## ログ

omnoothは起動ごとに、処理段階と成功・失敗を次の場所へ記録します。

```text
~/omnooth/logs/log_YYYYMMDD_HHMMSS.nnnnnnnnn.txt
```

コマンドが失敗した場合は、標準エラーにも該当するログファイルのパスを表示します。Custom URLや署名付きダウンロードURLなどの機密情報はログへ記録しません。
