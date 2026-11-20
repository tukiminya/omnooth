use gpui::{Context, Div, FontWeight, prelude::*, px, rgb};

use crate::{
    Omnooth,
    dropdown::{DropdownId, MenuAction, MenuEntry, MenuTrigger},
    ui::{
        BORDER,
        ButtonStyle::{Outline, Primary},
        CONTENT_INSET, CONTROL_HEIGHT, HOVER, MONO_FONT, SELECTED, button, column, divider, field,
        icon, label, muted, row, scroll_area,
    },
};

const FILE_STATE: DropdownId = ("booth-file-state", 0);
const FILE_ORDER: DropdownId = ("booth-file-order", 0);
const FILE_STATES: [(&str, Option<bool>); 3] = [
    ("保存状態：すべて", None),
    ("原本のみ", Some(false)),
    ("展開済み", Some(true)),
];
const FILE_ORDERS: [(&str, bool); 2] = [("取り込みが新しい順", true), ("取り込みが古い順", false)];

struct VisibleProduct {
    index: usize,
    files: Vec<usize>,
}

impl Omnooth {
    pub fn booth_library(&self, compact: bool, cx: &mut Context<Self>) -> Div {
        let state = FILE_STATES[self.dropdowns.choice(FILE_STATE)].1;
        let newest_first = FILE_ORDERS[self.dropdowns.choice(FILE_ORDER)].1;
        let products = visible_products(state, newest_first);
        let selected = visible_selection(&products, self.selected_file);
        let file_count: usize = products.iter().map(|product| product.files.len()).sum();

        column()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(self.library_toolbar(products.len(), file_count, cx))
            .child(
                row()
                    .items_start()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.saved_files(&products, selected, file_count, compact, cx))
                    .children(selected.map(|(product, file)| {
                        file_details(&PRODUCTS[product], &PRODUCTS[product].files[file], compact)
                    })),
            )
    }

    fn saved_files(
        &self,
        products: &[VisibleProduct],
        selection: Option<(usize, usize)>,
        file_count: usize,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        column()
            .flex_1()
            .min_w_0()
            .h_full()
            .pl(CONTENT_INSET)
            .pr(px(16.))
            .pb(px(16.))
            .gap(px(8.))
            .child(
                row()
                    .h(px(32.))
                    .flex_shrink_0()
                    .pl(px(40.))
                    .pr(px(12.))
                    .gap(px(12.))
                    .child(muted("保存ファイル", 12.).flex_1().min_w_0())
                    .children(file_columns(
                        compact,
                        "版（日付@SHA-256）",
                        "原本サイズ",
                        "保存状態",
                        true,
                    ))
                    .child(gpui::div().w(px(18.)).flex_shrink_0()),
            )
            .child(
                scroll_area("booth-files")
                    .gap(px(12.))
                    .children(products.iter().map(|visible| {
                        let product = &PRODUCTS[visible.index];
                        column()
                            .w_full()
                            .flex_shrink_0()
                            .child(self.product_heading(visible, cx))
                            .children(visible.files.iter().map(|&index| {
                                let file = &product.files[index];
                                let id = (visible.index, index);
                                let selected = selection == Some(id);
                                row()
                                    .id(file.name)
                                    .h(CONTROL_HEIGHT)
                                    .flex_shrink_0()
                                    .w_full()
                                    .px(px(12.))
                                    .gap(px(12.))
                                    .rounded(px(4.))
                                    .cursor_pointer()
                                    .when(selected, |item| item.bg(rgb(SELECTED)))
                                    .when(!selected, |item| {
                                        item.hover(|style| style.bg(rgb(HOVER)))
                                    })
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.selected_file = id;
                                        cx.notify();
                                    }))
                                    .child(
                                        row()
                                            .flex_1()
                                            .min_w_0()
                                            .gap(px(10.))
                                            .child(icon("icon/archive.svg"))
                                            .child(label(file.name, 14.).min_w_0().truncate()),
                                    )
                                    .children(file_columns(
                                        compact,
                                        file.version,
                                        file.size,
                                        file.state(),
                                        false,
                                    ))
                                    .child(icon("icon/file-detail.svg"))
                            }))
                    })),
            )
            .child(divider())
            .child(
                row()
                    .justify_between()
                    .gap(px(10.))
                    .child(muted(format!("{file_count} ファイルを表示"), 12.))
                    .child(muted("最終取り込み 2026-11-19 10:41", 12.)),
            )
    }

    fn library_toolbar(
        &self,
        product_count: usize,
        file_count: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        column()
            .w_full()
            .flex_shrink_0()
            .px(CONTENT_INSET)
            .pt(px(16.))
            .pb(px(12.))
            .gap(px(12.))
            .child(
                row()
                    .w_full()
                    .justify_between()
                    .child(
                        column()
                            .gap(px(4.))
                            .child(
                                row()
                                    .gap(px(16.))
                                    .child(
                                        label("保存済みアセット", 18.)
                                            .font_weight(FontWeight::SEMIBOLD),
                                    )
                                    .child(muted(
                                        format!("{product_count} 商品・{file_count} ファイル"),
                                        13.,
                                    )),
                            )
                            .child(muted(
                                "BOOTH から取り込んだ商品と配布ファイルを管理します。",
                                13.,
                            )),
                    )
                    .child(button(Outline, "ライブラリを開く")),
            )
            .child(
                row()
                    .w_full()
                    .gap(px(12.))
                    .child(
                        field()
                            .child(icon("icon/search.svg"))
                            .child(muted("商品名・ショップ名・ファイル名で検索", 14.).truncate()),
                    )
                    .child(self.select(FILE_STATE, 196., &FILE_STATES.map(|(label, _)| label), cx))
                    .child(self.select(FILE_ORDER, 212., &FILE_ORDERS.map(|(label, _)| label), cx))
                    .child(button(Outline, "お気に入り")),
            )
    }

    fn product_heading(&self, visible: &VisibleProduct, cx: &mut Context<Self>) -> Div {
        let product = &PRODUCTS[visible.index];
        row()
            .w_full()
            .h(px(64.))
            .gap(px(12.))
            .child(
                column()
                    .flex_1()
                    .min_w_0()
                    .gap(px(4.))
                    .child(
                        label(product.title, 16.)
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate(),
                    )
                    .child(
                        row()
                            .gap(px(8.))
                            .child(muted(product.shop, 12.))
                            .child(muted(
                                format!("{} ファイルを保存", product.files.len()),
                                12.,
                            )),
                    ),
            )
            .child(icon("icon/booth-link.svg"))
            .child(self.menu(
                ("booth-product-menu", visible.index),
                MenuTrigger::Product,
                &[
                    MenuEntry::new(
                        "ファイルの詳細を表示",
                        MenuAction::ShowFile((visible.index, visible.files[0])),
                    ),
                    MenuEntry::new("商品名をコピー", MenuAction::Copy(product.title)),
                ],
                cx,
            ))
    }
}

fn visible_products(state: Option<bool>, newest_first: bool) -> Vec<VisibleProduct> {
    let mut products: Vec<_> = PRODUCTS
        .iter()
        .enumerate()
        .filter_map(|(index, product)| {
            let mut files: Vec<_> = product
                .files
                .iter()
                .enumerate()
                .filter(|(_, file)| state.is_none_or(|extracted| file.extracted == extracted))
                .map(|(index, _)| index)
                .collect();
            files.sort_by_key(|&index| product.files[index].imported_at);
            if newest_first {
                files.reverse();
            }
            (!files.is_empty()).then_some(VisibleProduct { index, files })
        })
        .collect();

    products.sort_by_key(|group| PRODUCTS[group.index].files[group.files[0]].imported_at);
    if newest_first {
        products.reverse();
    }
    products
}

fn visible_selection(
    products: &[VisibleProduct],
    selected: (usize, usize),
) -> Option<(usize, usize)> {
    if products
        .iter()
        .any(|product| product.index == selected.0 && product.files.contains(&selected.1))
    {
        Some(selected)
    } else {
        products
            .first()
            .map(|product| (product.index, product.files[0]))
    }
}

fn file_columns(
    compact: bool,
    version: &'static str,
    size: &'static str,
    state: &'static str,
    heading: bool,
) -> [Div; 3] {
    let version_width = if compact { 144. } else { 160. };
    let state_width = if compact { 72. } else { 80. };
    let version = muted(version, 12.)
        .w(px(version_width))
        .flex_shrink_0()
        .when(!heading, |el| el.font_family(MONO_FONT));
    let size = muted(size, 13.).w(px(64.)).flex_shrink_0().text_right();
    let state = row().w(px(state_width)).flex_shrink_0().child(if heading {
        muted(state, 12.)
    } else {
        tag(state)
    });

    [version, size, state]
}

fn tag(text: &'static str) -> Div {
    label(text, 12.)
        .px(px(8.))
        .py(px(3.))
        .rounded(px(4.))
        .border_1()
        .border_color(rgb(BORDER))
}

fn file_details(product: &Product, file: &SavedFile, compact: bool) -> impl IntoElement {
    let hash = match file.name {
        "Eku_PC_v1_2_0.zip" => "8f3a21c04376f5a87b1920d34d6ac8bc\nd17c62a1e5bf4389609a72d8c114e6ab",
        _ => file.version.split_once('@').map_or("", |(_, hash)| hash),
    };

    column()
        .w(px(if compact { 292. } else { 352. }))
        .h_full()
        .border_l_1()
        .border_color(rgb(BORDER))
        .flex_shrink_0()
        .child(
            scroll_area("booth-file-details")
                .pl(px(20.))
                .pr(CONTENT_INSET)
                .pb(px(16.))
                .gap(px(14.))
                .child(
                    column()
                        .gap(px(6.))
                        .child(muted("選択したファイル", 12.))
                        .child(
                            row().gap(px(10.)).child(icon("icon/archive.svg")).child(
                                label(file.name, 16.)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate(),
                            ),
                        )
                        .child(muted(product.name, 14.).truncate())
                        .child(row().gap(px(8.)).child(tag("ZIP")).child(tag(file.state()))),
                )
                .child(
                    column()
                        .gap(px(8.))
                        .child(fact("版", file.version))
                        .child(fact("取り込み日時", file.imported_at))
                        .child(fact("原本サイズ", file.size))
                        .child(fact("原本の状態", "保存済み")),
                )
                .child(divider())
                .child(
                    column()
                        .gap(px(8.))
                        .child(label("原本と展開", 16.).font_weight(FontWeight::SEMIBOLD))
                        .child(muted(
                            "原本を保存しています。\nZIP は展開せずに内容を確認できます。",
                            13.,
                        ))
                        .child(
                            row()
                                .gap(px(8.))
                                .child(button(Primary, "内容を見る").flex_1().min_w_0())
                                .child(button(Outline, "展開する").px(px(12.))),
                        )
                        .child(button(Outline, "原本を表示").w_full()),
                )
                .child(divider())
                .child(
                    column()
                        .gap(px(8.))
                        .child(muted("ライブラリ保存先", 12.))
                        .child(
                            row()
                                .gap(px(8.))
                                .child(icon("icon/folder.svg"))
                                .child(label("~/omnooth/packages/booth", 13.).truncate()),
                        )
                        .child(muted("SHA-256", 12.))
                        .child(
                            row()
                                .gap(px(10.))
                                .child(muted(hash, 11.).font_family(MONO_FONT).flex_1().min_w_0())
                                .child(icon("icon/copy.svg")),
                        ),
                )
                .child(divider())
                .child(button(Outline, "BOOTH の商品ページ").w_full()),
        )
}

fn fact(name: &'static str, value: &'static str) -> Div {
    row()
        .gap(px(8.))
        .child(muted(name, 13.).w(px(100.)).flex_shrink_0())
        .child(label(value, 13.).min_w_0().truncate())
}

struct Product {
    name: &'static str,
    title: &'static str,
    shop: &'static str,
    files: &'static [SavedFile],
}

struct SavedFile {
    name: &'static str,
    version: &'static str,
    size: &'static str,
    extracted: bool,
    imported_at: &'static str,
}

impl SavedFile {
    fn state(&self) -> &'static str {
        if self.extracted {
            "展開済み"
        } else {
            "原本のみ"
        }
    }
}

const fn saved_file(
    name: &'static str,
    version: &'static str,
    size: &'static str,
    extracted: bool,
    imported_at: &'static str,
) -> SavedFile {
    SavedFile {
        name,
        version,
        size,
        extracted,
        imported_at,
    }
}

const PRODUCTS: [Product; 3] = [
    Product {
        name: "エク - Eku #Eku3D（PC版）",
        title: "【オリジナル3Dモデル】エク - Eku #Eku3D（PC版）",
        shop: "sep-neko-ya",
        files: &[
            saved_file(
                "Eku_PC_v1_2_0.zip",
                "2026-10-18@8f3a21c0",
                "1.19 GB",
                false,
                "2026-11-19 10:41",
            ),
            saved_file(
                "Eku_PC_v1_3_0.zip",
                "2026-11-18@c724b3e1",
                "1.24 GB",
                true,
                "2026-11-19 10:40",
            ),
        ],
    },
    Product {
        name: "Eclat & Crumb / エクラ & クラム",
        title: "【3D衣装】Eclat & Crumb / エクラ & クラム",
        shop: "sep-neko-ya",
        files: &[
            saved_file(
                "EclatAndCrumb_v1.1.0_Eku.zip",
                "2026-11-16@6b8034d2",
                "238 MB",
                false,
                "2026-11-19 10:39",
            ),
            saved_file(
                "EclatAndCrumb_v1.1.0_PSD.zip",
                "2026-11-16@b7a904c1",
                "512 MB",
                true,
                "2026-11-19 10:38",
            ),
            saved_file(
                "EclatAndCrumb_v1.1.0_Document.zip",
                "2026-11-16@02d6e7af",
                "2.4 MB",
                false,
                "2026-11-19 10:37",
            ),
        ],
    },
    Product {
        name: "和桜セーラー",
        title: "【21アバター対応】衣装モデル「和桜セーラー」",
        shop: "ぽなぺい",
        files: &[
            saved_file(
                "Wakasakura_sailor_eku.zip",
                "2026-11-12@946eb2a8",
                "184 MB",
                true,
                "2026-11-19 10:36",
            ),
            saved_file(
                "Textures.zip",
                "2026-11-12@d34a82c1",
                "46.2 MB",
                false,
                "2026-11-19 10:35",
            ),
            saved_file(
                "Wakasakura_sailor_karin.zip",
                "2026-11-12@ac4e0819",
                "181 MB",
                false,
                "2026-11-19 10:34",
            ),
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::{visible_products, visible_selection};

    #[test]
    fn filtering_keeps_the_selection_visible() {
        let products = visible_products(Some(true), true);
        assert_eq!(
            products
                .iter()
                .map(|product| product.files.len())
                .sum::<usize>(),
            3
        );
        assert_eq!(visible_selection(&products, (0, 0)), Some((0, 1)));
        assert_eq!(visible_selection(&products, (1, 1)), Some((1, 1)));
    }

    #[test]
    fn sorting_reorders_products_and_their_files() {
        let newest = visible_products(None, true);
        assert_eq!((newest[0].index, newest[0].files[0]), (0, 0));

        let oldest = visible_products(None, false);
        assert_eq!((oldest[0].index, oldest[0].files[0]), (2, 2));
        assert_eq!(oldest[2].files, [1, 0]);
    }
}
