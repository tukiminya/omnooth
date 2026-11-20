use gpui::{Context, Div, FontWeight, prelude::*, px};

use crate::{
    Omnooth,
    dropdown::DropdownId,
    ui::{
        ButtonStyle::{Outline, Primary},
        CONTENT_INSET, button, column, divider, field, label, muted, page_title, row, scroll_area,
    },
};

const THEME: DropdownId = ("theme", 0);
const THEMES: &[&str] = &["システムに合わせる", "ライト", "ダーク"];

impl Omnooth {
    pub fn settings(&self, cx: &mut Context<Self>) -> Div {
        column()
            .size_full()
            .px(CONTENT_INSET)
            .pt(px(24.))
            .pb(px(24.))
            .gap(px(18.))
            .child(
                column()
                    .gap(px(4.))
                    .child(page_title("Settings"))
                    .child(muted("表示とデフォルトの保存場所を設定します。", 13.)),
            )
            .child(
                scroll_area("settings-sections")
                    .gap(px(18.))
                    .child(
                        section("外観", "アプリの配色を切り替えます。").child(
                            row()
                                .h(px(52.))
                                .gap(px(24.))
                                .child(setting_name("テーマ"))
                                .child(self.select(THEME, 260., THEMES, cx))
                                .child(muted("ライト / ダーク / システムに合わせる", 13.).truncate()),
                        ),
                    )
                    .child(divider())
                    .child(
                        section("プロジェクト", "新しく作成するプロジェクトの保存先です。")
                            .child(path_row("アバター", "~/omnooth/projects/avatars"))
                            .child(path_row("ワールド", "~/omnooth/projects/worlds"))
                            .child(path_row("その他のプロジェクト", "~/omnooth/projects/others")),
                    )
                    .child(divider())
                    .child(
                        section("バックアップ", "プロジェクトのバックアップを保存します。")
                            .child(path_row("バックアップの保存場所", "~/omnooth/backups")),
                    )
                    .child(divider())
                    .child(
                        section("パッケージ", "ダウンロードしたパッケージとアセットを保存します。")
                            .child(path_row("VPM パッケージ", "~/omnooth/packages/vpm"))
                            .child(path_row("BOOTH Assets", "~/omnooth/packages/booth"))
                            .child(
                                row()
                                    .min_h(px(60.))
                                    .gap(px(12.))
                                    .child(self.checkbox(("extract-zip", 0), false, cx))
                                    .child(
                                        column()
                                            .flex_1()
                                            .min_w_0()
                                            .gap(px(2.))
                                            .child(label("ZIP ファイルを自動で展開する", 16.))
                                            .child(muted("ON にすると、取り込み時に展開します。OFF の場合も、必要なときに手動で展開できます。", 13.)),
                                    ),
                            ),
                    ),
            )
            .child(divider())
            .child(
                row()
                    .flex_shrink_0()
                    .gap(px(12.))
                    .child(
                        muted("保存先の変更は、これから作成・取り込むデータに適用されます。", 13.)
                            .flex_1()
                            .min_w_0(),
                    )
                    .child(button(Outline, "変更を破棄").id("reset-settings").cursor_pointer().on_click(
                        cx.listener(|view, _, _, cx| {
                            view.changed_checks.remove(&("extract-zip", 0));
                            view.dropdowns.reset(THEME);
                            cx.notify();
                        }),
                    ))
                    .child(button(Primary, "設定を保存")),
            )
    }
}

fn section(title: &'static str, detail: &'static str) -> Div {
    column().gap(px(8.)).flex_shrink_0().child(
        row()
            .gap(px(16.))
            .child(label(title, 18.).font_weight(FontWeight::SEMIBOLD))
            .child(muted(detail, 13.)),
    )
}

fn path_row(name: &'static str, path: &'static str) -> Div {
    row()
        .h(px(52.))
        .gap(px(24.))
        .child(setting_name(name))
        .child(field().child(label(path, 14.).min_w_0().truncate()))
        .child(button(Outline, "参照…"))
}

fn setting_name(name: &'static str) -> Div {
    label(name, 16.).w(px(260.)).flex_shrink_0()
}
