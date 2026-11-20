# Omnooth

Unity プロジェクトとアセットの管理画面を確認する GPUI 製デスクトッププロトタイプです。[Figma の omnooth v2](https://www.figma.com/design/aKMF6XjaOU2Hz7Bj9GxPXY/omnooth?node-id=197-669) を参照しています。

## 起動

リポジトリのルートで実行します。macOS では Rust と Xcode が必要です。

```sh
cargo run --locked -p omnooth
```

左のナビゲーションから Projects、Packages、Templates、Logs、Settings を開けます。Projects の「管理」からプロジェクト詳細に入り、Packages と Assets を切り替えられます。Packages の BOOTH Assets では保存ファイルを選択すると右ペインに詳細が表示されます。

表示には固定サンプルを使います。画面遷移、ファイル選択、チェック状態はウィンドウ内で操作できます。Unity の起動やパッケージの適用、ファイル処理、設定の保存などは未実装です。

## 構成

ルートの `Cargo.toml` が Cargo ワークスペースを定義します。現在のメンバーは `apps/omnooth` の 1 パッケージです。

- `apps/omnooth/src/main.rs` はウィンドウと画面遷移を管理します。
- `apps/omnooth/src/ui.rs` は共通の表示部品を定義し、各画面はそれぞれのモジュールに置いています。
- `apps/omnooth/assets/` の SVG とフォントは実行ファイルに埋め込みます。アイコンは `assets/icon/` にまとめています。[Lucide のライセンス](apps/omnooth/assets/icon/LICENSE-Lucide.txt) と、`assets/fonts/` の各フォントのライセンスを同梱しています。Omnooth のワードマークは `assets/brand/wordmark.svg` にあります。[ブランドロゴの権利表記](apps/omnooth/assets/brand/LICENSE-Brand.txt) を参照してください。
