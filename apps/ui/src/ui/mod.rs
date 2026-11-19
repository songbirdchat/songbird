//! Songbird's `ui` layer: every styled control the features compose.
//! Features import from here, never controls from `nsh_base` directly,
//! and never write colours, radii or font sizes of their own.

pub mod button;
pub mod calls_controls;
pub mod chat_menu;
pub mod controls;
pub mod extras_parts;
pub mod floating;
pub mod groups_press;
pub mod icons;
pub mod onboarding_kit;
pub mod parts;
pub mod portrait;
pub mod qr;
pub mod settings_controls;
pub mod skin;
pub mod tokens;

pub use button::{BtnSize, Variant, btn, btn_with_icon, fab, icon_btn};
pub use calls_controls::{CallTone, call_btn, call_pill};
pub use chat_menu::{app_menu, menu_entry, menu_rule, pressable};
pub use controls::{
    ToastInset, ToastState, app_header, app_sheet, ask_first, confirm, field, labelled_field, page,
    page_with, search_field, segmented, set_toast_inset, share_text, toast, toast_layer, toggle,
    toggle_row, toggle_row_icon, toggle_row_with,
};
pub use extras_parts::{bullet, key_cap, link, select_card};
pub use groups_press::{Press, press_surface};
pub use icons::{Icon, icon};
pub use onboarding_kit::{
    MONO, StatusTone, busy_bar, code_cells, flow_layout, flow_lead, flow_title, hero_icon,
    key_blocks, numbered_step, numeric_code, page_dots, passcode_cells, progress_bar, scanner,
    status_line, step_segments,
};
pub use parts::{
    Availability, EDGE_FADE, ICON_TILE, INSET_AVATAR, INSET_ICON, INSET_PLAIN, Leading, Photo, Row,
    Tick, Tone, Trailing, avatar, avatar_of, avatar_presence, avatar_presence_on, banner, body,
    card, chip, chip_scroller, edge_fade, empty_state, footnote, group, group_avatar, group_plain,
    group_with_inset, hairline, heading, hint, icon_tile, initials, muted, page_scroll,
    presence_badge, section_title, separator, separator_inset, skeleton, skeleton_row, tag, ticks,
    unread_badge, unread_badge_on, wide,
};
pub use qr::{qr_code, qr_code_live};
pub use settings_controls::{callout, expander, option_pill, radio_row, swatch, tile};
pub use skin::{Skin, SkinSpec, skin, spec, with_skin};
pub use tokens::{TOUCH, c, font, motion, radius};

#[cfg(test)]
mod tests;
