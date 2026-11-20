mod assets;
mod booth;
mod dropdown;
mod logs;
mod packages;
mod projects;
mod settings;
mod ui;

use std::collections::HashSet;

use assets::Assets;
use dropdown::Dropdowns;
use gpui::{
    App, Application, Bounds, Context, KeyBinding, KeystrokeEvent, Menu, MenuItem, MouseButton,
    TitlebarOptions, Window, WindowBounds, WindowOptions, actions, div, point, prelude::*, px, rgb,
    size,
};
use ui::{BACKGROUND, BORDER, TEXT, column, icon, label, row, svg_asset};

actions!(omnooth, [Quit]);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Projects,
    ProjectPackages,
    ProjectAssets,
    Repositories,
    UserPackages,
    BoothAssets,
    Templates,
    Logs,
    Settings,
}

impl Screen {
    fn section(self) -> Self {
        match self {
            Self::ProjectPackages | Self::ProjectAssets => Self::Projects,
            Self::UserPackages | Self::BoothAssets => Self::Repositories,
            _ => self,
        }
    }
}

const NAVIGATION: [(Screen, &str, &str); 5] = [
    (Screen::Projects, "Projects", "icon/projects.svg"),
    (Screen::Repositories, "Packages", "icon/packages.svg"),
    (Screen::Templates, "Templates", "icon/templates.svg"),
    (Screen::Logs, "Logs", "icon/logs.svg"),
    (Screen::Settings, "Settings", "icon/settings.svg"),
];

struct Omnooth {
    screen: Screen,
    show_sidebar: bool,
    selected_file: (usize, usize),
    changed_checks: HashSet<(&'static str, usize)>,
    dropdowns: Dropdowns,
}

impl Default for Omnooth {
    fn default() -> Self {
        Self {
            screen: Screen::Projects,
            show_sidebar: true,
            selected_file: (0, 0),
            changed_checks: HashSet::new(),
            dropdowns: Dropdowns::default(),
        }
    }
}

impl Omnooth {
    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        row()
            .h(px(43.))
            .w_full()
            .flex_shrink_0()
            .pl(px(if cfg!(target_os = "macos") { 88. } else { 16. }))
            .gap(px(12.))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                row()
                    .id("toggle-sidebar")
                    .h_full()
                    .cursor_pointer()
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.dropdowns.close();
                        view.show_sidebar = !view.show_sidebar;
                        cx.notify();
                    }))
                    .child(icon("icon/menu.svg")),
            )
            .child(svg_asset("brand/wordmark.svg").w(px(67.)).h(px(12.)))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
            )
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        column()
            .w(px(200.))
            .h_full()
            .flex_shrink_0()
            .justify_between()
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(column().children(NAVIGATION.map(|(screen, title, glyph)| {
                self.navigation_item(screen, title, glyph, self.screen.section() == screen, cx)
                    .w_full()
            })))
            .child(
                row()
                    .h(px(34.))
                    .px(px(16.))
                    .gap(px(6.))
                    .child(label(concat!("Ver ", env!("CARGO_PKG_VERSION")), 10.))
                    .child(svg_asset("icon/copy-small.svg").size(px(10.))),
            )
    }

    fn navigation_item(
        &self,
        screen: Screen,
        title: &'static str,
        glyph: &'static str,
        active: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        row()
            .id((title, screen as usize))
            .h(px(43.))
            .px(px(16.))
            .gap(px(12.))
            .cursor_pointer()
            .when(active, |item| item.bg(rgb(ui::SELECTED)))
            .when(!active, |item| item.hover(|style| style.bg(rgb(ui::HOVER))))
            .on_click(cx.listener(move |view, _, _, cx| {
                view.dropdowns.close();
                view.screen = screen;
                cx.notify();
            }))
            .child(icon(glyph))
            .child(label(title, 16.))
    }

    fn tabs(
        &self,
        items: &[(Screen, &'static str, &'static str)],
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        row()
            .w_full()
            .flex_shrink_0()
            .border_b_1()
            .border_color(rgb(BORDER))
            .children(items.iter().map(|&(screen, title, glyph)| {
                self.navigation_item(screen, title, glyph, self.screen == screen, cx)
                    .flex_1()
                    .justify_center()
            }))
    }

    fn checkbox(
        &self,
        id: (&'static str, usize),
        initial_value: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        ui::checkbox(initial_value ^ self.changed_checks.contains(&id))
            .id(id)
            .cursor_pointer()
            .on_click(cx.listener(move |view, _, _, cx| {
                if !view.changed_checks.remove(&id) {
                    view.changed_checks.insert(id);
                }
                cx.notify();
            }))
    }
}

impl Render for Omnooth {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar_width = if self.show_sidebar { px(200.) } else { px(0.) };
        let content_width = window.viewport_size().width - sidebar_width;
        let page = match self.screen {
            Screen::Projects => self.projects(content_width < px(960.), cx),
            Screen::ProjectPackages | Screen::ProjectAssets => self.project_detail(cx),
            Screen::Repositories | Screen::UserPackages | Screen::BoothAssets => {
                self.packages(content_width < px(1200.), cx)
            }
            Screen::Templates => self.templates(cx),
            Screen::Logs => self.logs(content_width < px(1100.), cx),
            Screen::Settings => self.settings(cx),
        };

        column()
            .tab_group()
            .size_full()
            .overflow_hidden()
            .bg(rgb(BACKGROUND))
            .text_color(rgb(TEXT))
            .font_family(ui::UI_FONT)
            .text_size(px(16.))
            .line_height(px(24.))
            .child(self.header(cx))
            .child(
                row()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .items_start()
                    .when(self.show_sidebar, |body| body.child(self.sidebar(cx)))
                    .child(column().size_full().flex_1().min_w_0().child(page)),
            )
    }
}

fn handle_tab(event: &KeystrokeEvent, window: &mut Window, cx: &mut App) {
    let modifiers = event.keystroke.modifiers;
    if event.keystroke.key != "tab"
        || modifiers.control
        || modifiers.alt
        || modifiers.platform
        || modifiers.function
    {
        return;
    }
    if let Some(Some(view)) = window.root::<Omnooth>() {
        view.update(cx, |view, cx| {
            if view.dropdowns.close() {
                cx.notify();
            }
        });
        if modifiers.shift {
            window.focus_prev();
        } else {
            window.focus_next();
        }
        window.prevent_default();
        cx.stop_propagation();
    }
}

fn main() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        if let Err(error) = cx.text_system().add_fonts(Assets::fonts()) {
            eprintln!("Could not load Omnooth's fonts: {error}");
        }

        let quit_key = if cfg!(target_os = "macos") {
            "cmd-q"
        } else {
            "ctrl-q"
        };
        cx.bind_keys([KeyBinding::new(quit_key, Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.intercept_keystrokes(handle_tab).detach();
        cx.set_menus(vec![Menu {
            name: "Omnooth".into(),
            items: vec![MenuItem::action("Quit Omnooth", Quit)],
        }]);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let titlebar = TitlebarOptions {
            title: Some("Omnooth".into()),
            appears_transparent: cfg!(target_os = "macos"),
            traffic_light_position: Some(point(px(12.), px(13.))),
        };

        cx.open_window(
            WindowOptions {
                titlebar: Some(titlebar),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(1000.), px(620.))),
                app_id: Some("dev.tuki.omnooth".into()),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Omnooth::default()),
        )
        .expect("Could not open the Omnooth window");
        cx.activate(true);
    });
}
