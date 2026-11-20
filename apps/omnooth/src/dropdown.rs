use std::{collections::HashMap, rc::Rc};

use gpui::{
    ClipboardItem, Context, Corner, Div, ElementId, MouseButton, Pixels, anchored, deferred, div,
    point, prelude::*, px, rgb,
};

use crate::{
    Omnooth, Screen,
    ui::{
        BORDER, CONTROL_HEIGHT, HOVER, MUTED, SELECTED, column, icon_button, label, row,
        select_trigger, svg_asset,
    },
};

pub type DropdownId = (&'static str, usize);

#[derive(Default)]
pub struct Dropdowns {
    open: Option<OpenDropdown>,
    choices: HashMap<DropdownId, usize>,
}

#[derive(Clone, Copy)]
struct OpenDropdown {
    id: DropdownId,
    active: usize,
}

impl Dropdowns {
    pub fn choice(&self, id: DropdownId) -> usize {
        self.choices.get(&id).copied().unwrap_or(0)
    }

    pub fn reset(&mut self, id: DropdownId) {
        self.choices.remove(&id);
    }

    pub fn close(&mut self) -> bool {
        self.open.take().is_some()
    }
}

#[derive(Clone, Copy)]
pub enum MenuAction {
    Select(usize),
    Navigate(Screen),
    Copy(&'static str),
    ShowFile((usize, usize)),
}

pub enum MenuTrigger {
    Bordered,
    Ellipsis,
    Product,
}

impl MenuTrigger {
    fn render(self) -> (Div, Pixels) {
        let (glyph, icon_size) = match self {
            Self::Bordered => return (icon_button("icon/more-vertical.svg"), CONTROL_HEIGHT),
            Self::Ellipsis => ("icon/more.svg", px(24.)),
            Self::Product => ("icon/product-menu.svg", px(18.)),
        };
        let height = px(24.);
        let trigger = row()
            .size(height)
            .justify_center()
            .rounded(px(4.))
            .child(svg_asset(glyph).size(icon_size));
        (trigger, height)
    }
}

#[derive(Clone, Copy)]
pub struct MenuEntry {
    text: &'static str,
    action: Option<MenuAction>,
}

impl MenuEntry {
    pub fn new(text: &'static str, action: MenuAction) -> Self {
        Self {
            text,
            action: Some(action),
        }
    }

    pub fn disabled(text: &'static str) -> Self {
        Self { text, action: None }
    }
}

impl Omnooth {
    pub fn select(
        &self,
        id: DropdownId,
        width: f32,
        options: &[&'static str],
        cx: &mut Context<Self>,
    ) -> Div {
        let selected = self.dropdowns.choice(id);
        let items = options
            .iter()
            .enumerate()
            .map(|(index, &text)| MenuEntry::new(text, MenuAction::Select(index)))
            .collect();
        self.dropdown(
            id,
            (select_trigger(options[selected], width), CONTROL_HEIGHT),
            items,
            Some(selected),
            px(width),
            cx,
        )
    }

    pub fn menu(
        &self,
        id: DropdownId,
        trigger: MenuTrigger,
        items: &[MenuEntry],
        cx: &mut Context<Self>,
    ) -> Div {
        self.dropdown(id, trigger.render(), items.into(), None, px(208.), cx)
    }

    fn dropdown(
        &self,
        id: DropdownId,
        (trigger, trigger_height): (Div, Pixels),
        items: Rc<[MenuEntry]>,
        selected: Option<usize>,
        menu_width: Pixels,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.dropdowns.open.filter(|open| open.id == id);
        let initial = selected.or_else(|| items.iter().position(|entry| entry.action.is_some()));
        let keyboard_items = items.clone();
        let trigger = trigger
            .id(id)
            .tab_index(0)
            .cursor_pointer()
            .hover(|style| style.bg(rgb(HOVER)))
            .focus(|style| style.bg(rgb(HOVER)))
            .when(open.is_some(), |trigger| trigger.bg(rgb(HOVER)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _, _, cx| {
                    // Outside-click dismissal runs before the trigger's mouse-down handler.
                    if open.is_some() {
                        view.dropdowns.close();
                    } else if let Some(active) = initial {
                        view.dropdowns.open = Some(OpenDropdown { id, active });
                    }
                    cx.notify();
                }),
            )
            .on_key_down(
                cx.listener(move |view, event: &gpui::KeyDownEvent, window, cx| {
                    if !event.keystroke.modifiers.modified() {
                        let current = view.dropdowns.open.filter(|open| open.id == id);
                        match (event.keystroke.key.as_str(), current) {
                            ("up" | "down", Some(open)) => {
                                let forward = event.keystroke.key == "down";
                                let active = next_enabled(&keyboard_items, open.active, forward);
                                view.dropdowns.open = Some(OpenDropdown { id, active });
                            }
                            ("enter" | "space", Some(open)) if !event.is_held => {
                                view.activate_menu_entry(id, keyboard_items[open.active], cx);
                            }
                            ("escape", Some(_)) => {
                                view.dropdowns.close();
                            }
                            ("enter" | "space" | "up" | "down", None) if !event.is_held => {
                                view.dropdowns.open =
                                    initial.map(|active| OpenDropdown { id, active });
                            }
                            _ => return,
                        }
                        window.prevent_default();
                        cx.stop_propagation();
                        cx.notify();
                    }
                }),
            );

        div()
            .relative()
            .flex_shrink_0()
            .child(trigger)
            .when_some(open, |control, open| {
                let popup = column()
                    .id((ElementId::from(id), "popup"))
                    .w(menu_width)
                    .p(px(4.))
                    .rounded(px(8.))
                    .bg(rgb(0x343739))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .shadow_lg()
                    .occlude()
                    .on_mouse_down_out(cx.listener(move |view, _, _, cx| {
                        if view.dropdowns.open.is_some_and(|open| open.id == id) {
                            view.dropdowns.close();
                            cx.notify();
                        }
                    }))
                    .children(items.iter().copied().enumerate().map(|(index, entry)| {
                        let checked = selected == Some(index);
                        let item = row()
                            .h(px(34.))
                            .px(px(10.))
                            .gap(px(8.))
                            .rounded(px(4.))
                            .child(
                                label(entry.text, 14.)
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .when(entry.action.is_none(), |label| {
                                        label.text_color(rgb(MUTED))
                                    }),
                            )
                            .when(checked, |item| {
                                item.child(svg_asset("icon/check.svg").size(px(16.)))
                            })
                            .when(open.active == index, |item| item.bg(rgb(SELECTED)));
                        item.id(index).when(entry.action.is_some(), |item| {
                            item.cursor_pointer()
                                .hover(|style| style.bg(rgb(SELECTED)))
                                .on_hover(cx.listener(move |view, hovered, _, cx| {
                                    if *hovered
                                        && let Some(open) = view.dropdowns.open.as_mut()
                                        && open.id == id
                                        && open.active != index
                                    {
                                        open.active = index;
                                        cx.notify();
                                    }
                                }))
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.activate_menu_entry(id, entry, cx);
                                }))
                        })
                    }));
                control.child(
                    div().absolute().right_0().top_0().child(deferred(
                        anchored()
                            .anchor(Corner::TopRight)
                            .offset(point(px(0.), trigger_height + px(5.)))
                            .child(popup),
                    )),
                )
            })
    }

    fn activate_menu_entry(&mut self, id: DropdownId, entry: MenuEntry, cx: &mut Context<Self>) {
        let Some(open) = self.dropdowns.open else {
            return;
        };
        let Some(action) = entry.action else { return };
        if open.id != id {
            return;
        }

        self.dropdowns.close();
        match action {
            MenuAction::Select(index) => {
                self.dropdowns.choices.insert(id, index);
            }
            MenuAction::Navigate(screen) => self.screen = screen,
            MenuAction::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text.into())),
            MenuAction::ShowFile(file) => self.selected_file = file,
        }
        cx.notify();
    }
}

fn next_enabled(items: &[MenuEntry], active: usize, forward: bool) -> usize {
    let count = items.len();
    (1..count)
        .map(|step| (active + if forward { step } else { count - step }) % count)
        .find(|&index| items[index].action.is_some())
        .unwrap_or(active)
}
