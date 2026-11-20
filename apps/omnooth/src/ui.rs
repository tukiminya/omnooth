use gpui::{
    Div, ElementId, FontWeight, Pixels, SharedString, Stateful, Svg, div, prelude::*, px, rgb, svg,
};

pub const UI_FONT: &str = "Gen Interface JP";
pub const MONO_FONT: &str = "JetBrains Mono";

pub const CONTENT_INSET: Pixels = px(32.);
pub const CONTROL_HEIGHT: Pixels = px(42.);
pub const LIST_ROW_HEIGHT: Pixels = px(58.);

pub const BACKGROUND: u32 = 0x414447;
pub const SELECTED: u32 = 0x65696d;
pub const HOVER: u32 = 0x505457;
pub const BORDER: u32 = 0x5a5a5a;
pub const BUTTON_BORDER: u32 = 0x7a7a7a;
pub const TEXT: u32 = 0xffffff;
pub const SECONDARY: u32 = 0xeaeaea;
pub const MUTED: u32 = 0xbababa;
pub const BUTTON_TEXT: u32 = 0x1a1a1a;
pub const REMOVE: u32 = 0xff5c5c;

pub fn row() -> Div {
    div().flex().items_center()
}

pub fn column() -> Div {
    div().flex().flex_col()
}

pub fn label(text: impl Into<SharedString>, font_size: f32) -> Div {
    div()
        .text_size(px(font_size))
        .line_height(px((font_size * 1.45).round()))
        .child(text.into())
}

pub fn muted(text: impl Into<SharedString>, font_size: f32) -> Div {
    label(text, font_size).text_color(rgb(MUTED))
}

pub fn svg_asset(path: &'static str) -> Svg {
    svg().path(path).text_color(rgb(TEXT)).flex_shrink_0()
}

pub fn icon(path: &'static str) -> Svg {
    svg_asset(path).size(px(18.))
}

pub fn divider() -> Div {
    div().h(px(1.)).w_full().flex_shrink_0().bg(rgb(BORDER))
}

pub fn scroll_area(id: impl Into<ElementId>) -> Stateful<Div> {
    column()
        .id(id)
        .flex_1()
        .min_h_0()
        .w_full()
        .overflow_y_scroll()
}

pub fn page_title(title: &'static str) -> Div {
    label(title, 20.).font_weight(FontWeight::SEMIBOLD)
}

pub fn page_heading(title: &'static str) -> Div {
    row()
        .h(CONTROL_HEIGHT + px(20.))
        .px(CONTENT_INSET)
        .w_full()
        .flex_shrink_0()
        .justify_between()
        .child(page_title(title))
}

pub fn description(name: &'static str, subtitle: &'static str) -> Div {
    column()
        .flex_1()
        .min_w_0()
        .gap(px(2.))
        .child(label(name, 16.).truncate())
        .child(muted(subtitle, 12.).truncate())
}

#[derive(Clone, Copy)]
pub enum ButtonStyle {
    Primary,
    Outline,
}

pub fn button(style: ButtonStyle, text: &'static str) -> Div {
    button_frame(style).child(label(text, 16.).whitespace_nowrap())
}

pub fn button_with_icon(style: ButtonStyle, text: &'static str, glyph: &'static str) -> Div {
    button_frame(style)
        .child(icon(glyph).text_color(rgb(button_text(style))))
        .child(label(text, 16.).whitespace_nowrap())
}

pub fn icon_button(glyph: &'static str) -> Div {
    button_frame(ButtonStyle::Outline)
        .w(CONTROL_HEIGHT)
        .px_0()
        .child(icon(glyph))
}

fn button_frame(style: ButtonStyle) -> Div {
    row()
        .h(CONTROL_HEIGHT)
        .px(px(18.))
        .gap(px(10.))
        .flex_shrink_0()
        .justify_center()
        .rounded(px(8.))
        .text_color(rgb(button_text(style)))
        .font_weight(FontWeight::SEMIBOLD)
        .when(matches!(style, ButtonStyle::Primary), |el| el.bg(rgb(TEXT)))
        .when(matches!(style, ButtonStyle::Outline), |el| {
            el.border_2().border_color(rgb(BUTTON_BORDER))
        })
}

fn button_text(style: ButtonStyle) -> u32 {
    match style {
        ButtonStyle::Primary => BUTTON_TEXT,
        ButtonStyle::Outline => TEXT,
    }
}

pub fn select_trigger(text: &'static str, width: f32) -> Div {
    button_frame(ButtonStyle::Outline)
        .w(px(width))
        .justify_between()
        .gap(px(10.))
        .font_weight(FontWeight::NORMAL)
        .child(label(text, 16.).truncate())
        .child(svg_asset("icon/chevron-down-small.svg").size(px(14.)))
}

pub fn field() -> Div {
    row()
        .flex_1()
        .min_w_0()
        .h(CONTROL_HEIGHT)
        .px(px(16.))
        .gap(px(10.))
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(BORDER))
}

pub fn checkbox(checked: bool) -> Div {
    row()
        .size(px(22.))
        .flex_shrink_0()
        .justify_center()
        .rounded(px(8.))
        .border_2()
        .border_color(rgb(BORDER))
        .when(checked, |el| {
            el.bg(rgb(BORDER))
                .child(svg_asset("icon/check.svg").size(px(16.)))
        })
}

pub fn list_row() -> Div {
    row()
        .w_full()
        .h(LIST_ROW_HEIGHT)
        .flex_shrink_0()
        .px(CONTENT_INSET)
        .gap(px(16.))
}

pub fn remove_icon() -> Svg {
    svg_asset("icon/remove.svg")
        .size(px(24.))
        .text_color(rgb(REMOVE))
}
