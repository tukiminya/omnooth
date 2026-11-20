use gpui::{
    Context, Div, FontWeight, Hsla, linear_color_stop, linear_gradient, prelude::*, px, rgb,
};

use crate::{
    Omnooth, Screen,
    dropdown::{MenuAction, MenuEntry, MenuTrigger},
    ui::{
        BACKGROUND,
        ButtonStyle::{Outline, Primary},
        CONTENT_INSET, CONTROL_HEIGHT, HOVER, LIST_ROW_HEIGHT, MUTED, SECONDARY, button,
        button_with_icon, column, description, icon, label, list_row, muted, page_heading,
        remove_icon, row, scroll_area, svg_asset,
    },
};

const PROJECT_PATH: &str = "~/omnooth/eku";

impl Omnooth {
    pub fn projects(&self, compact: bool, cx: &mut Context<Self>) -> Div {
        column().size_full().child(page_heading("Projects")).child(
            scroll_area("projects").children((0_usize..2).map(|index| {
                let identity = row()
                    .flex_1()
                    .min_w_0()
                    .gap(px(12.))
                    .child(icon("icon/star.svg"))
                    .child(
                        column()
                            .min_w_0()
                            .child(label("Eku", 18.).font_weight(FontWeight::SEMIBOLD))
                            .child(
                                label(PROJECT_PATH, 14.)
                                    .text_color(rgb(SECONDARY))
                                    .truncate(),
                            ),
                    );
                let metadata = row()
                    .gap(px(16.))
                    .flex_shrink_0()
                    .child(
                        row()
                            .gap(px(8.))
                            .child(icon("icon/avatar.svg"))
                            .child(label("アバター", 14.)),
                    )
                    .child(label("2022.3.22f1", 14.))
                    .child(label("2026-10-18", 14.))
                    .child(label("1ヶ月前", 14.));
                let actions = row()
                    .gap(px(12.))
                    .flex_shrink_0()
                    .child(button(Primary, "Unity を開く"))
                    .child(
                        button(Outline, "管理")
                            .id(("manage-project", index))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(HOVER)))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.dropdowns.close();
                                view.screen = Screen::ProjectPackages;
                                cx.notify();
                            })),
                    )
                    .child(button(Outline, "バックアップ"))
                    .child(self.menu(
                        ("project-menu", index),
                        MenuTrigger::Bordered,
                        &[
                            MenuEntry::new("管理", MenuAction::Navigate(Screen::ProjectPackages)),
                            MenuEntry::new("パスをコピー", MenuAction::Copy(PROJECT_PATH)),
                            MenuEntry::disabled("バックアップ"),
                        ],
                        cx,
                    ));

                let body = column()
                    .w_full()
                    .flex_shrink_0()
                    .min_h(LIST_ROW_HEIGHT)
                    .px(CONTENT_INSET)
                    .py(px(6.))
                    .gap(px(4.));
                let main_row = row().gap(px(16.)).child(identity);

                if compact {
                    body.child(main_row.child(actions))
                        .child(metadata.pl(px(30.)))
                } else {
                    body.child(main_row.child(metadata).child(actions))
                }
            })),
        )
    }

    pub fn project_detail(&self, cx: &mut Context<Self>) -> Div {
        let tabs = [
            (Screen::ProjectPackages, "Packages", "icon/packages.svg"),
            (Screen::ProjectAssets, "Assets", "icon/assets.svg"),
        ];
        column()
            .size_full()
            .child(
                page_heading("Eku")
                    .child(
                        label(PROJECT_PATH, 14.)
                            .text_color(rgb(SECONDARY))
                            .ml(px(16.))
                            .flex_1()
                            .min_w_0()
                            .truncate(),
                    )
                    .child(
                        row()
                            .gap(px(12.))
                            .child(button(Primary, "Unity を開く"))
                            .child(button(Outline, "バックアップ"))
                            .child(self.menu(
                                ("project-detail-menu", 0),
                                MenuTrigger::Bordered,
                                &[
                                    MenuEntry::new(
                                        "一覧に戻る",
                                        MenuAction::Navigate(Screen::Projects),
                                    ),
                                    MenuEntry::new("パスをコピー", MenuAction::Copy(PROJECT_PATH)),
                                    MenuEntry::disabled("バックアップ"),
                                ],
                                cx,
                            )),
                    ),
            )
            .child(self.tabs(&tabs, cx))
            .child(if self.screen == Screen::ProjectPackages {
                self.project_packages(cx).into_any_element()
            } else {
                self.project_assets(cx).into_any_element()
            })
    }

    fn project_packages(&self, cx: &mut Context<Self>) -> impl IntoElement {
        scroll_area("project-packages").children(PROJECT_PACKAGES.iter().enumerate().map(
            |(index, &(name, id, installed, available))| {
                let mut versions = vec![installed];
                if available != installed {
                    versions.push(available);
                }
                if installed != "なし" {
                    versions.push("なし");
                }
                list_row()
                    .child(self.checkbox(("project-package", index), false, cx))
                    .child(
                        description(name, id)
                            .when(installed == "なし", |entry| entry.text_color(rgb(MUTED))),
                    )
                    .child(self.select(("project-package-version", index), 160., &versions, cx))
                    .child(row().w(px(112.)).justify_center().child(
                        if installed != "なし" && installed != available {
                            button_with_icon(Primary, available, "icon/update.svg")
                                .w_full()
                                .px(px(12.))
                        } else {
                            label(available, 16.)
                        },
                    ))
                    .child(if installed == "なし" {
                        svg_asset("icon/install.svg").size(px(24.))
                    } else {
                        remove_icon()
                    })
                    .child(self.menu(
                        ("project-package-menu", index),
                        MenuTrigger::Ellipsis,
                        &[MenuEntry::new(
                            "パッケージ ID をコピー",
                            MenuAction::Copy(id),
                        )],
                        cx,
                    ))
            },
        ))
    }

    fn project_assets(&self, cx: &mut Context<Self>) -> impl IntoElement {
        scroll_area("project-assets")
            .px(CONTENT_INSET)
            .py(px(18.))
            .gap(px(24.))
            .children(PROJECT_ASSETS.iter().map(|asset| {
                column()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .gap(px(6.))
                    .child(label(asset.title, 16.).truncate())
                    .child(muted(asset.shop, 12.))
                    .children(asset.files.iter().map(|file| self.project_file(file, cx)))
            }))
    }

    fn project_file(&self, file: &ProjectFile, cx: &mut Context<Self>) -> Div {
        let (style, text, glyph) = match file.action {
            FileAction::Install | FileAction::More => {
                (Outline, "インストール", "icon/cloud-download.svg")
            }
            FileAction::Add => (Primary, "追加", "icon/add.svg"),
            FileAction::Update => (Primary, "更新する", "icon/update.svg"),
        };
        let is_more = matches!(file.action, FileAction::More);

        row()
            .relative()
            .w_full()
            .h(CONTROL_HEIGHT)
            .gap(px(12.))
            .child(if is_more {
                crate::ui::checkbox(false).into_any_element()
            } else {
                self.checkbox((file.name, 0), file.checked, cx)
                    .into_any_element()
            })
            .child(label(file.name, 14.).flex_1().min_w_0().truncate())
            .child(
                label("2026-11-19", 14.)
                    .w(px(112.))
                    .flex_shrink_0()
                    .text_center(),
            )
            .child(button_with_icon(style, text, glyph).w(px(150.)).px(px(10.)))
            .when(is_more, |line| {
                let background = Hsla::from(rgb(BACKGROUND));
                line.child(
                    row()
                        .absolute()
                        .inset_0()
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(background.opacity(0.4), 0.),
                            linear_color_stop(background, 1.),
                        ))
                        .justify_center()
                        .gap(px(8.))
                        .child(label("残り 20 ファイル", 14.))
                        .child(icon("icon/chevron-down.svg")),
                )
            })
    }
}

const PROJECT_PACKAGES: [(&str, &str, &str, &str); 7] = [
    ("VRChat SDK - Base", "com.vrchat.base", "3.10.5", "3.10.6"),
    (
        "VRChat SDK - Avatars",
        "com.vrchat.avatars",
        "3.10.5",
        "3.10.6",
    ),
    ("lilToon", "jp.lilxyzw.liltoon", "2.3.4", "2.4.1"),
    (
        "Modular Avatar",
        "nadena.dev.modular-avatar",
        "1.18.7",
        "1.18.7",
    ),
    (
        "Non-Destructive Modular Framework",
        "nadena.dev.ndmf",
        "1.14.8",
        "1.15.0",
    ),
    (
        "VRChat Package Resolver Tool",
        "com.vrchat.core.vpm-resolver",
        "なし",
        "0.1.29",
    ),
    ("AudioLink", "com.llealloo.audiolink", "なし", "3.1.2"),
];

struct ProjectAsset {
    title: &'static str,
    shop: &'static str,
    files: &'static [ProjectFile],
}

struct ProjectFile {
    name: &'static str,
    action: FileAction,
    checked: bool,
}

enum FileAction {
    Install,
    Add,
    Update,
    More,
}

const fn file(name: &'static str, action: FileAction, checked: bool) -> ProjectFile {
    ProjectFile {
        name,
        action,
        checked,
    }
}

const PROJECT_ASSETS: [ProjectAsset; 3] = [
    ProjectAsset {
        title: "【オリジナル3Dモデル】エク - Eku #Eku3D（PC版）",
        shop: "sep-neko-ya",
        files: &[
            file("Eku_PC_v1_2_0.zip", FileAction::Install, false),
            file(
                "sepnekoya-ofc-Eku_MochiFitter_Profile_v0_2_0.zip",
                FileAction::Install,
                false,
            ),
            file("AsteriaLycee_forEku_v1_3_0.zip", FileAction::Add, false),
            file("Eku_PC_v1_3_0.zip", FileAction::Update, false),
            file("AsteriaLycee_forEku_v1_1_0.zip", FileAction::Install, false),
        ],
    },
    ProjectAsset {
        title: "【3D衣装】Eclat & Crumb（複数アバター対応）#EclatCrumb（エク［共通素体：ミルフィ］）",
        shop: "sep-neko-ya",
        files: &[
            file("EclatAndCrumb_v1_1_0_PSD.zip", FileAction::Add, false),
            file("EclatAndCrumb_v1_1_0_Eku.zip", FileAction::Add, false),
            file(
                "EclatAndCrumb_v1_1_0_Document.zip",
                FileAction::Install,
                false,
            ),
        ],
    },
    ProjectAsset {
        title: "【21アバター対応】衣装モデル『和桜セーラー』 (フルセット)",
        shop: "ほむぺじ",
        files: &[
            file("Textures.zip", FileAction::Add, true),
            file("Wasakura_sailor_eku.zip", FileAction::Add, true),
            file("Wasakura_sailor_karin.zip", FileAction::More, false),
        ],
    },
];
