use gpui::{Context, Div, div, prelude::*, px};

use crate::{
    Omnooth, Screen,
    dropdown::{MenuAction, MenuEntry, MenuTrigger},
    ui::{
        ButtonStyle::Primary, button, button_with_icon, column, description, label, list_row,
        page_heading, remove_icon, scroll_area, svg_asset,
    },
};

const REPOSITORY_URL: &str = "https://packages.vrchat.com/official?download";
const USER_PACKAGE_PATH: &str = "~/omnooth/templates/abc";

impl Omnooth {
    pub fn packages(&self, compact: bool, cx: &mut Context<Self>) -> Div {
        let actions = match self.screen {
            Screen::Repositories => Some(button_with_icon(Primary, "新規追加", "icon/add.svg")),
            Screen::BoothAssets => Some(button_with_icon(
                Primary,
                "ファイルを取り込む",
                "icon/add.svg",
            )),
            _ => None,
        };
        let tabs = [
            (
                Screen::Repositories,
                "VPM Repositories",
                "icon/packages.svg",
            ),
            (
                Screen::UserPackages,
                "User Packages",
                "icon/user-packages.svg",
            ),
            (Screen::BoothAssets, "BOOTH Assets", "icon/assets.svg"),
        ];

        column()
            .size_full()
            .child(page_heading("Packages").children(actions))
            .child(self.tabs(&tabs, cx))
            .child(match self.screen {
                Screen::Repositories => self.repositories(cx).into_any_element(),
                Screen::UserPackages => self.user_packages(cx).into_any_element(),
                _ => self.booth_library(compact, cx).into_any_element(),
            })
    }

    fn repositories(&self, cx: &mut Context<Self>) -> impl IntoElement {
        scroll_area("repositories").children((0..2).map(|index| {
            list_row()
                .child(svg_asset("icon/drag.svg").size(px(20.)))
                .child(self.checkbox(("repository", index), false, cx))
                .child(description("Official", REPOSITORY_URL))
                .child(remove_icon())
                .child(self.menu(
                    ("repository-menu", index),
                    MenuTrigger::Ellipsis,
                    &[
                        MenuEntry::new("URL をコピー", MenuAction::Copy(REPOSITORY_URL)),
                        MenuEntry::disabled("削除"),
                    ],
                    cx,
                ))
        }))
    }

    fn user_packages(&self, cx: &mut Context<Self>) -> impl IntoElement {
        scroll_area("user-packages").children((0..2).map(|index| {
            list_row()
                .child(self.checkbox(("user-package", index), false, cx))
                .child(description("abc", USER_PACKAGE_PATH))
                .child(
                    label("0.1.0", 16.)
                        .w(px(120.))
                        .flex_shrink_0()
                        .text_center(),
                )
                .child(div().size(px(24.)).flex_shrink_0())
                .child(self.menu(
                    ("user-package-menu", index),
                    MenuTrigger::Ellipsis,
                    &[
                        MenuEntry::new("パスをコピー", MenuAction::Copy(USER_PACKAGE_PATH)),
                        MenuEntry::disabled("削除"),
                    ],
                    cx,
                ))
        }))
    }

    pub fn templates(&self, cx: &mut Context<Self>) -> Div {
        column()
            .size_full()
            .child(page_heading("Templates").child(button_with_icon(
                Primary,
                "新規追加",
                "icon/add.svg",
            )))
            .child(
                scroll_area("templates").children(
                    ["Avatar", "Eku", "Honoka", "Cian", "Milfy"]
                        .into_iter()
                        .enumerate()
                        .map(|(index, name)| {
                            list_row()
                                .child(self.checkbox(("template", index), false, cx))
                                .child(label(name, 16.).flex_1())
                                .child(button(Primary, "管理"))
                                .child(remove_icon())
                                .child(self.menu(
                                    ("template-menu", index),
                                    MenuTrigger::Ellipsis,
                                    &[
                                        MenuEntry::new("名前をコピー", MenuAction::Copy(name)),
                                        MenuEntry::disabled("削除"),
                                    ],
                                    cx,
                                ))
                        }),
                ),
            )
    }
}
