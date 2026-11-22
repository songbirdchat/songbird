//! Rows, sections, avatars, badges, ticks, chips, banners and the small
//! pieces every screen is made of.

use nsh_base::button;
use nsh_core::prelude::*;

use super::{
    icons::{Icon, icon},
    skin::{Elevation, PressStyle, SeparatorStyle, Skin, spec},
    tokens::{c, font, motion, radius},
};

// ---------------------------------------------------------------- layout

/// Whether the window takes the desktop layout (sidebar + panes). By
/// width, so a narrow desktop window gets the phone layout and a wide
/// tablet the desktop one. Reading it in a binding subscribes to the size.
pub fn wide() -> bool {
    window().width >= if is_desktop() { px(760.0) } else { px(900.0) }
}

/// A list row's minimum height: 44 on desktops (Fluent's compact 40,
/// Adwaita's 50), 56 on phones.
pub fn row_height() -> f32 {
    match (super::skin::skin(), is_desktop()) {
        (super::skin::Skin::Fluent, true) => 40.0,
        (super::skin::Skin::Adwaita, true) => 50.0,
        (_, true) => 44.0,
        (_, false) => 56.0,
    }
}

/// A screen's scrolling body with the standard gutters.
pub fn page_scroll(content: impl IntoElement) -> Scroll {
    // Apple's phone tab bar floats over the content: leave room to
    // scroll the last item clear of it.
    let floating = spec().nav == super::skin::NavChrome::GlassCapsule && !wide();
    scroll()
        .flex_1()
        .keyboard_dismiss_mode(KeyboardDismissMode::OnDrag)
        .child(
            view()
                .pb_safe_area_plus(px(if floating { 24.0 + FLOATING_BAR } else { 24.0 }))
                .child(content),
        )
}

/// How much of the window's bottom Apple's floating tab bar covers
/// (its height and the gap under it), for content that scrolls beneath.
pub const FLOATING_BAR: f32 = 74.0;

/// One device pixel, in points: the width of every hairline (separators,
/// pane borders, card outlines), so a rule is as thin as the screen can
/// draw it at 1x, 2x and 3x alike.
pub fn hairline() -> Points {
    px(1.0 / pixel_ratio().max(1.0))
}

/// The hint an icon-only control shows on desktops while the pointer
/// rests on it or the keyboard focuses it (`.tooltip(hint("Calls"))`):
/// the toast's inverted surface at caption size, the popup radius.
pub fn hint(label: impl Into<SharedString>) -> nsh_base::TooltipElement {
    nsh_base::tooltip().child(
        view()
            .px_2()
            .py_1()
            .rounded(move || radius::control())
            .bg(move || c::foreground())
            .shadow_md()
            .child(
                text(label.into())
                    .number_of_lines(1)
                    .text_size(px(font::CAPTION))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(move || c::background()),
            ),
    )
}

/// A thin rule (a hairline).
pub fn separator() -> View {
    view().h(hairline()).bg(move || c::border())
}

/// An inset rule that starts after a leading avatar or icon.
pub fn separator_inset(left: f32) -> View {
    view().h(hairline()).ml(px(left)).bg(move || c::border())
}

/// Where a group's separators start: under the row titles. A row's
/// gutter (16) plus, with a leading icon tile, the tile (30) and the gap
/// (12) to its title.
pub const INSET_PLAIN: f32 = 16.0;
/// See [`INSET_PLAIN`]: rows that lead with an [`icon_tile`].
pub const INSET_ICON: f32 = 16.0 + ICON_TILE + 12.0;
/// See [`INSET_PLAIN`]: rows that lead with a 36pt avatar.
pub const INSET_AVATAR: f32 = 16.0 + 36.0 + 12.0;

/// The side of [`icon_tile`].
pub const ICON_TILE: f32 = 30.0;

// ---------------------------------------------------------------- text

/// A section title (screen readers can jump between them).
pub fn section_title(label: impl IntoReactive<SharedString>) -> Text {
    let sp = spec();
    // Apple: small caps-style uppercase in the secondary colour; Material:
    // the list subheader in primary; Fluent and Adwaita: a strong title.
    let t = text(label).role(AccessibilityRole::Header);
    let t = match sp.skin {
        Skin::Apple => t
            .text_size(px(font::SMALL))
            .uppercase()
            .letter_spacing(px(0.3))
            .text_color(move || c::muted_fg()),
        Skin::Material => t
            .text_size(px(14.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(move || c::primary()),
        Skin::Fluent => t
            .text_size(px(14.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(move || c::foreground()),
        Skin::Adwaita => t
            .text_size(px(15.0))
            .font_weight(FontWeight::BOLD)
            .text_color(move || c::foreground()),
    };
    t.px_4().pt_5().pb_2()
}

/// Muted help text under a group.
pub fn footnote(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_size(px(font::SMALL))
        .line_height(px(18.0))
        .text_color(move || c::muted_fg())
        // Margins, not padding: padding on a Text is measured into its
        // wrap width and clipped its last line.
        .mx_4()
        .mt_2()
}

/// A page heading for big empty or intro screens.
pub fn heading(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_size(px(font::DISPLAY))
        .line_height(px(34.0))
        .font_weight(FontWeight::BOLD)
        .role(AccessibilityRole::Header)
}

/// Body text of the platform size.
pub fn body(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_size(px(font::body()))
        .line_height(px(font::body() * 1.4))
}

/// Muted secondary text.
pub fn muted(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .text_size(px(font::SMALL + 1.0))
        .line_height(px(20.0))
        .text_color(move || c::muted_fg())
}

// ---------------------------------------------------------------- groups

/// A grouped list of rows on a card (settings, chat info). Its
/// separators start under the titles of rows that lead with an icon tile
/// ([`INSET_ICON`]); a group of plain rows takes [`group_plain`], one of
/// avatar rows [`group_with_inset`] with [`INSET_AVATAR`].
pub fn group(rows: impl IntoIterator<Item = AnyElement>) -> View {
    group_with_inset(rows, INSET_ICON)
}

/// A group of rows without a leading icon: separators start at the
/// gutter, under the titles.
pub fn group_plain(rows: impl IntoIterator<Item = AnyElement>) -> View {
    group_with_inset(rows, INSET_PLAIN)
}

/// A group whose separators start `inset` points from its leading edge
/// (one of the `INSET_*` constants).
pub fn group_with_inset(rows: impl IntoIterator<Item = AnyElement>, inset: f32) -> View {
    let rows: Vec<AnyElement> = rows.into_iter().collect();
    let count = rows.len();
    let sp = spec();
    let r = sp.radius.card;
    // Material 3 Expressive: a segmented list, each row its own tonal
    // tile, 2pt apart, the group's outer corners large and the inner ones
    // small. The others: one card, rows split by the skin's separators.
    if sp.separator == SeparatorStyle::None {
        return view()
            .mx_4()
            .gap(px(2.0))
            .children(rows.into_iter().enumerate().map(move |(i, row)| {
                view()
                    .overflow_hidden()
                    .bg(move || c::card())
                    .rounded(px(4.0))
                    .when(i == 0, |v| v.rounded_t(px(r)))
                    .when(i + 1 == count, |v| v.rounded_b(px(r)))
                    .child(row)
            }));
    }
    let line = if sp.separator == SeparatorStyle::Full {
        0.0
    } else {
        inset
    };
    view()
        .mx_4()
        .rounded(px(r))
        .overflow_hidden()
        .bg(move || c::card())
        .when(sp.card_stroke, |v| {
            v.border(if sp.skin == Skin::Fluent {
                px(1.0)
            } else {
                hairline()
            })
            .border_color(move || c::border())
        })
        .children(rows.into_iter().enumerate().map(move |(i, row)| {
            view()
                .child(row)
                .child((i + 1 < count).then(|| separator_inset(line)))
        }))
}

/// What a row leads with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leading {
    None,
    /// An [`icon_tile`].
    Icon,
    /// Anything else (an avatar): its width is the caller's.
    Element,
}

impl Leading {
    /// The separator inset under a row with this leading: [`INSET_PLAIN`],
    /// [`INSET_ICON`], or [`INSET_AVATAR`] for an element (36pt avatars).
    pub fn inset(self) -> f32 {
        match self {
            Leading::None => INSET_PLAIN,
            Leading::Icon => INSET_ICON,
            Leading::Element => INSET_AVATAR,
        }
    }
}

/// What a row shows at its end.
pub enum Trailing {
    None,
    Chevron,
    Value(SharedString),
    Element(AnyElement),
}

/// The standard row: an optional leading icon or element, a title, an
/// optional subtitle, a trailing value or chevron. Pressable when
/// `on_press` is given; `danger` makes it red.
pub struct Row {
    leading: Option<AnyElement>,
    leading_kind: Leading,
    title: SharedString,
    subtitle: Option<SharedString>,
    trailing: Trailing,
    danger: bool,
    on_press: Option<Box<dyn Fn()>>,
}

impl Row {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            leading: None,
            leading_kind: Leading::None,
            title: title.into(),
            subtitle: None,
            trailing: Trailing::None,
            danger: false,
            on_press: None,
        }
    }
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.leading = Some(icon_tile(glyph).into_element());
        self.leading_kind = Leading::Icon;
        self
    }
    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_element());
        self.leading_kind = Leading::Element;
        self
    }
    /// What the row leads with, to pick its group's separator inset
    /// ([`Leading::inset`]).
    pub fn leading_kind(&self) -> Leading {
        self.leading_kind
    }
    pub fn subtitle(mut self, s: impl Into<SharedString>) -> Self {
        self.subtitle = Some(s.into());
        self
    }
    pub fn chevron(mut self) -> Self {
        self.trailing = Trailing::Chevron;
        self
    }
    pub fn value(mut self, v: impl Into<SharedString>) -> Self {
        self.trailing = Trailing::Value(v.into());
        self
    }
    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing = Trailing::Element(el.into_element());
        self
    }
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    pub fn on_press(mut self, f: impl Fn() + 'static) -> Self {
        self.on_press = Some(Box::new(f));
        self
    }

    pub fn build(self) -> AnyElement {
        let danger = self.danger;
        let pressable = self.on_press.is_some();
        let ink = move || {
            if danger {
                c::destructive()
            } else {
                c::foreground()
            }
        };
        let content = view()
            .flex_row()
            .items_center()
            .gap_3()
            .px_4()
            .min_h(px(row_height()))
            .py_2()
            .child(self.leading)
            .child(
                view()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(2.0))
                    .child(
                        text(self.title.clone())
                            .text_size(px(font::body()))
                            .text_color(ink)
                            .number_of_lines(2),
                    )
                    .child(self.subtitle.clone().map(|s| {
                        text(s)
                            .text_size(px(font::SMALL))
                            .text_color(move || c::muted_fg())
                            .number_of_lines(3)
                    })),
            )
            .child(match self.trailing {
                Trailing::None => None,
                // Optical: the chevron's stroke ends 6pt inside its 18pt
                // box; pulling the box 4pt out puts the stroke on the
                // trailing spine with values, tags and switches.
                Trailing::Chevron => Some(
                    icon(Icon::ChevronRight, 18.0)
                        .mr(px(-4.0))
                        .text_color(move || c::muted_fg())
                        .into_element(),
                ),
                Trailing::Value(v) => Some(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(
                            text(v)
                                .text_size(px(font::SMALL + 1.0))
                                .text_color(move || c::muted_fg())
                                .number_of_lines(1),
                        )
                        // A chevron only where the row leads somewhere.
                        .child(pressable.then(|| {
                            icon(Icon::ChevronRight, 16.0)
                                .mr(px(-4.0))
                                .text_color(move || c::muted_fg())
                        }))
                        .into_element(),
                ),
                Trailing::Element(el) => Some(el),
            });
        let press = spec().press;
        match self.on_press {
            Some(f) => button()
                .on_press(move || f())
                .accessibility_label(self.title)
                .cursor_pointer()
                .transition_colors()
                .duration(motion::INSTANT)
                .hover(move |s| s.bg(move || row_wash(press, false)))
                .active(move |s| s.bg(move || row_wash(press, true)))
                .focus_visible(|s| s.bg(move || c::accent()))
                .child(content)
                .into_element(),
            None => content.into_element(),
        }
    }
}

/// The wash a pressable row takes when hovered or pressed: Apple's grey
/// highlight, Material's on-surface state layer (8% hover, 12% press),
/// Fluent's and Adwaita's subtle fill shift.
pub fn row_wash(press: PressStyle, pressed: bool) -> AdaptiveColor {
    match (press, pressed) {
        (PressStyle::Highlight, false) => c::muted(),
        (PressStyle::Highlight, true) => c::foreground().opacity(0.1),
        (PressStyle::Ripple, false) => c::foreground().opacity(0.08),
        (PressStyle::Ripple, true) => c::foreground().opacity(0.12),
        (PressStyle::Fill, false) => c::foreground().opacity(0.05),
        (PressStyle::Fill, true) => c::foreground().opacity(0.03),
    }
}

/// What holds a row's icon. Apple: Songbird's lavender squircle with the
/// glyph in the tint (Settings' tiles, in the brand); Material, Fluent and
/// Adwaita lists show the bare glyph in the content's colour. The box
/// keeps the same width, so separator insets hold, and callers may still
/// set its fill and ink (a danger tile).
pub fn icon_tile(glyph: Icon) -> View {
    let sp = spec();
    let small = sp.radius.small;
    match sp.skin {
        Skin::Apple => view()
            .size(px(ICON_TILE))
            .rounded(px(small))
            .items_center()
            .justify_center()
            .bg(move || c::accent())
            .text_color(move || c::primary())
            .child(icon(glyph, 17.0)),
        Skin::Material => view()
            .size(px(ICON_TILE))
            .items_center()
            .justify_center()
            .text_color(move || c::muted_fg())
            .child(icon(glyph, 22.0)),
        Skin::Fluent | Skin::Adwaita => view()
            .size(px(ICON_TILE))
            .items_center()
            .justify_center()
            .text_color(move || c::foreground())
            .child(icon(
                glyph,
                if sp.skin == Skin::Fluent { 20.0 } else { 18.0 },
            )),
    }
}

// ---------------------------------------------------------------- people

/// The initials of a name, at most two letters.
pub fn initials(name: &str) -> String {
    let mut out: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().next().filter(|c| c.is_alphanumeric()))
        .take(2)
        .collect();
    if out.is_empty() {
        out.push('?');
    }
    out.to_uppercase()
}

/// A round avatar: the person's profile photo (mock portraits for now,
/// `ui::portrait`), or their initials on their hue when they have none.
/// Decorative for screen readers: the row or header beside it names them.
pub fn avatar(name: impl Into<SharedString>, seed: u64, size: f32) -> View {
    let name: SharedString = name.into();
    if let Some(src) = crate::backend::avatar::photo(seed) {
        return view()
            .size(px(size))
            .rounded_full()
            .overflow_hidden()
            .accessibility_elements_hidden(true)
            .child(image(src).size(px(size)).object_fit(ResizeMode::Cover));
    }
    avatar_of(
        name,
        seed,
        size,
        if super::portrait::has_photo(seed) {
            Photo::Person
        } else {
            Photo::None
        },
    )
}

/// A group's, community's or channel's avatar: its photo (a mock scene),
/// or initials.
pub fn group_avatar(name: impl Into<SharedString>, seed: u64, size: f32) -> View {
    let name: SharedString = name.into();
    avatar_of(
        name,
        seed,
        size,
        if super::portrait::has_photo(seed + 1000) {
            Photo::Group
        } else {
            Photo::None
        },
    )
}

/// Which picture an avatar shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Photo {
    None,
    Person,
    Group,
}

/// An avatar showing `photo`, initials when there is none.
pub fn avatar_of(name: SharedString, seed: u64, size: f32, photo: Photo) -> View {
    let base = view()
        .size(px(size))
        .rounded_full()
        .overflow_hidden()
        .items_center()
        .justify_center()
        .bg(move || c::avatar(seed))
        .accessibility_elements_hidden(true);
    let face = match photo {
        Photo::None => base.child(
            text(initials(&name))
                .text_size(px((size * 0.4).max(11.0)))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || c::on_avatar()),
        ),
        Photo::Person | Photo::Group => base.child(
            image(if photo == Photo::Person {
                super::portrait::portrait(seed)
            } else {
                super::portrait::scenery(seed)
            })
            .size(px(size))
            .object_fit(ResizeMode::Cover),
        ),
    };
    // The same hairline ring on every avatar, photo or initials, on every
    // surface: it keeps light photos from melting into light surfaces and
    // gives all avatars one edge.
    face.child(
        view()
            .absolute()
            .inset_0()
            .rounded_full()
            .border(hairline())
            .border_color(move || c::foreground())
            .opacity(0.1),
    )
}

/// An avatar with the online dot at its corner, on the page background.
pub fn avatar_presence(
    name: impl Into<SharedString>,
    seed: u64,
    size: f32,
    online: impl IntoReactive<bool> + 'static,
) -> View {
    avatar_presence_on(name, seed, size, online, c::background)
}

/// [`avatar_presence`] on another surface: the dot's ring takes
/// `surface`'s colour (`c::card` in a group, `c::sidebar` on the rail),
/// so it reads as a cut-out of the avatar.
pub fn avatar_presence_on(
    name: impl Into<SharedString>,
    seed: u64,
    size: f32,
    online: impl IntoReactive<bool> + 'static,
    surface: fn() -> AdaptiveColor,
) -> View {
    let online = online.into_reactive();
    // Whole points so the dot and its ring land on the pixel grid.
    let dot = (size * 0.28).max(10.0).round();
    view().size(px(size)).child(avatar(name, seed, size)).child(
        view()
            .absolute()
            .right(px(0.0))
            .bottom(px(0.0))
            .size(px(dot))
            .rounded_full()
            .bg(move || c::online())
            .border_2()
            .border_color(move || surface())
            .opacity(move || if online.get() { 1.0_f32 } else { 0.0 }),
    )
}

/// What a presence mark says. Never colour alone: each has its own shape,
/// and the row or control around it says the word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Online,
    Away,
    Busy,
}

impl Availability {
    pub fn label(self) -> &'static str {
        match self {
            Availability::Online => "online",
            Availability::Away => "away",
            Availability::Busy => "busy",
        }
    }
}

/// A presence mark for an avatar's corner (place it with `absolute()`):
/// a filled dot (online), a ring (away), a dot crossed by a bar (busy),
/// cut out of `surface` by a 2pt ring of its colour. Fades in and out
/// (`BASE`, kept with reduced motion: a fade, not a move); hidden from
/// screen readers, since the row says it in words.
pub fn presence_badge(
    kind: impl Fn() -> Option<Availability> + Copy + 'static,
    dot: f32,
    surface: fn() -> AdaptiveColor,
) -> View {
    let dot = dot.round();
    let is = move |a: Availability| kind() == Some(a);
    view()
        .p(px(2.0))
        .rounded_full()
        .bg(move || surface())
        .opacity(move || if kind().is_some() { 1.0_f32 } else { 0.0 })
        .transition_opacity()
        .duration(motion::BASE)
        .accessibility_elements_hidden(true)
        .child(
            view()
                .size(px(dot))
                .rounded_full()
                .items_center()
                .justify_center()
                .bg(move || match kind() {
                    Some(Availability::Online) => c::online(),
                    Some(Availability::Busy) => c::destructive(),
                    _ => AdaptiveColor::TRANSPARENT,
                })
                .border_2()
                .border_color(move || {
                    if is(Availability::Away) {
                        c::warning()
                    } else {
                        AdaptiveColor::TRANSPARENT
                    }
                })
                .child(
                    view()
                        .w(px((dot * 0.55).round()))
                        .h(px(2.0))
                        .rounded_full()
                        .bg(move || c::on_destructive())
                        .style_when(move || !is(Availability::Busy), |st| st.hidden()),
                ),
        )
}

// ---------------------------------------------------------------- badges

/// An unread counter pill. `muted` greys it (a muted chat).
pub fn unread_badge(count: impl Fn() -> u32 + 'static, muted: impl Fn() -> bool + 'static) -> View {
    let count = std::rc::Rc::new(count);
    let shown = count.clone();
    let muted = std::rc::Rc::new(muted);
    let m2 = muted.clone();
    view()
        .min_w(px(20.0))
        .h(px(20.0))
        .px(px(6.0))
        .rounded_full()
        .items_center()
        .justify_center()
        .bg(move || if muted() { c::muted_fg() } else { c::primary() })
        .child(
            text(move || {
                let n = shown();
                if n > 999 {
                    "999+".to_string()
                } else {
                    n.to_string()
                }
            })
            .text_size(px(font::CAPTION))
            .font_weight(FontWeight::BOLD)
            .tabular_nums()
            .text_color(move || {
                if m2() {
                    c::background()
                } else {
                    c::on_primary()
                }
            }),
        )
        .label(move || format!("{} unread", count()))
}

/// An [`unread_badge`] cut out of `surface` by a 2pt ring of its colour,
/// for a badge that sits over an icon (the tab bar, the rail).
pub fn unread_badge_on(count: impl Fn() -> u32 + 'static, surface: fn() -> AdaptiveColor) -> View {
    view()
        .p(px(2.0))
        .rounded_full()
        .bg(move || surface())
        .child(unread_badge(count, || false))
}

/// A short neutral label (role, "Admin", "Plus", "WhatsApp").
pub fn tag(label: impl IntoReactive<SharedString>) -> View {
    view()
        .px(px(6.0))
        .py(px(2.0))
        .rounded(move || radius::sm())
        .bg(move || c::secondary())
        .child(
            text(label)
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || c::on_secondary()),
        )
}

// ---------------------------------------------------------------- ticks

/// Message delivery status. It only moves forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tick {
    Sending,
    Sent,
    Delivered,
    Read,
    Played,
    Failed,
}

impl Tick {
    pub fn label(self) -> &'static str {
        match self {
            Tick::Sending => "Sending",
            Tick::Sent => "Sent",
            Tick::Delivered => "Delivered",
            Tick::Read => "Read",
            Tick::Played => "Played",
            Tick::Failed => "Not sent",
        }
    }
}

/// The status mark. Shapes differ, not just colour: a clock, one tick,
/// two ticks, two ticks in a filled circle (read), two ticks in a smaller
/// filled circle with two sound waves beside it (played: heard, not a
/// control), and a warning triangle (failed).
pub fn ticks(status: impl Fn() -> Tick + Copy + 'static) -> View {
    let mark = |t: Tick| -> &'static str {
        match t {
            Tick::Sending => r#"<circle cx="12" cy="12" r="8"/><path d="M12 8v4l2.5 1.5"/>"#,
            Tick::Sent => r#"<path d="M5 12.5l4.5 4.5L19 7.5"/>"#,
            Tick::Delivered => {
                r#"<path d="M2 12.5l4.5 4.5L16 7.5"/><path d="M10 15.5l1.5 1.5L21 7.5"/>"#
            }
            Tick::Read => {
                r#"<circle cx="12" cy="12" r="11" fill="currentColor" stroke="none"/><path d="M4.5 12.5l3.5 3.5 7-7.5" stroke="white"/><path d="M10.5 15l1 1 7-7.5" stroke="white"/>"#
            }
            Tick::Played => {
                r#"<circle cx="9" cy="12" r="8.5" fill="currentColor" stroke="none"/><g transform="translate(9 12) scale(0.72) translate(-12 -12)" stroke="white" stroke-width="2.6"><path d="M4.5 12.5l3.5 3.5 7-7.5"/><path d="M10.5 15l1 1 7-7.5"/></g><path d="M19.2 9a4.5 4.5 0 0 1 0 6" stroke-width="1.8"/><path d="M21.9 6.6a8 8 0 0 1 0 10.8" stroke-width="1.8"/>"#
            }
            Tick::Failed => {
                r#"<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>"#
            }
        }
    };
    let src = move || {
        nsh_core::element::ImageSrc::svg_markup(&format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
            mark(status())
        ))
    };
    view()
        .label(move || status().label())
        .role(AccessibilityRole::Image)
        .child(svg(src).size(px(16.0)).text_color(move || match status() {
            Tick::Read | Tick::Played => c::tick_read(),
            Tick::Failed => c::destructive(),
            _ => c::bubble_meta(),
        }))
}

// ---------------------------------------------------------------- chips

/// A filter chip (chat list filters). Selected chips fill.
pub fn chip(
    label: impl Into<SharedString>,
    selected: impl Fn() -> bool + Copy + 'static,
    on_press: impl Fn() + 'static,
) -> nsh_base::ButtonElement {
    let label: SharedString = label.into();
    let sp = spec();
    // Material: an outlined filter chip that fills and gains a check when
    // chosen; Fluent: a toggle button that turns accent; Apple and
    // Adwaita: a soft fill that turns lavender.
    let material = sp.skin == Skin::Material;
    let fluent = sp.skin == Skin::Fluent;
    let height = match (material, is_desktop()) {
        (true, _) => 32.0,
        (false, true) => {
            if fluent {
                28.0
            } else {
                32.0
            }
        }
        (false, false) => 38.0,
    };
    button()
        .on_press(on_press)
        .selected(selected)
        .accessibility_label(label.clone())
        .h(px(height))
        .px_3()
        .flex_row()
        .gap(px(6.0))
        .items_center()
        .justify_center()
        .rounded(px(sp.radius.chip))
        .cursor_pointer()
        .bg(move || match (selected(), material, fluent) {
            (true, _, true) => c::primary(),
            (true, _, false) => c::accent(),
            (false, true, _) => AdaptiveColor::TRANSPARENT,
            (false, _, true) => c::control(),
            _ => c::secondary(),
        })
        .when(material || fluent, |b| {
            b.border_1()
                .border_color(move || match (selected(), material) {
                    (true, _) => AdaptiveColor::TRANSPARENT,
                    (false, true) => c::input(),
                    (false, false) => c::border(),
                })
        })
        .transition_colors()
        .duration(motion::INSTANT)
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.7))
        .focus_visible(|s| {
            s.outline(px(2.0))
                .outline_offset(px(2.0))
                .outline_color(move || c::ring())
        })
        .child(material.then(|| {
            icon(Icon::Check, 16.0)
                .text_color(move || c::on_accent())
                .style_when(
                    move || !selected(),
                    |st| st.hidden().border_color(AdaptiveColor::TRANSPARENT),
                )
        }))
        .child(
            text(label)
                .text_size(px(if material { 14.0 } else { font::SMALL + 1.0 }))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || match (selected(), fluent) {
                    (true, true) => c::on_primary(),
                    (true, false) => c::on_accent(),
                    (false, _) if material => c::foreground(),
                    _ => c::muted_fg(),
                }),
        )
}

/// A horizontal strip of chips (filters, lists) that scrolls. Its content
/// keeps the 16pt gutter at the start and leaves room for the fade at the
/// end, and a fade of `surface`'s colour at the trailing edge says more
/// chips are there instead of cutting the last one off at the pane's edge.
pub fn chip_scroller(
    chips: impl IntoIterator<Item = AnyElement>,
    surface: fn() -> AdaptiveColor,
) -> View {
    view()
        .flex_grow(0.0)
        .w_full()
        .min_w(px(0.0))
        .overflow_hidden()
        .child(
            scroll().horizontal().scrollbar(false).w_full().child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .pl_4()
                    .pr(px(EDGE_FADE + 8.0))
                    .py_2()
                    .role(AccessibilityRole::Tablist)
                    .children(chips),
            ),
        )
        .child(edge_fade(surface))
}

/// The width of [`edge_fade`].
pub const EDGE_FADE: f32 = 32.0;

/// A fade at the trailing edge of a horizontal scroller, from clear to
/// `surface`'s colour: put it last in a view around the scroll (that view
/// should be `w_full()` and `overflow_hidden()` so the fade sits at the
/// visible edge). It draws above the scroller (`z_index`), solid for its
/// last third so a chip under it is hidden, not hard-cut.
pub fn edge_fade(surface: fn() -> AdaptiveColor) -> View {
    view()
        .absolute()
        .top(px(0.0))
        .bottom(px(0.0))
        .right(px(0.0))
        .w(px(EDGE_FADE))
        .z_index(2)
        .pointer_events(PointerEvents::None)
        .accessibility_elements_hidden(true)
        .bg_linear_to_r()
        .gradient_from(move || surface().opacity(0.0))
        .gradient_via(move || surface().opacity(0.85))
        .gradient_via_position(Percentage(0.6))
        .gradient_to(move || surface())
}

// ---------------------------------------------------------------- banners

/// The tone of a banner.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Warning,
    Danger,
    Secure,
}

/// A calm, full-width notice with an icon (offline, key changed, the
/// encryption notice at the top of a chat).
pub fn banner(tone: Tone, glyph: Icon, message: impl IntoReactive<SharedString>) -> View {
    view()
        .flex_row()
        .items_start()
        .gap_2()
        .px_3()
        .py_2()
        .rounded(move || radius::control())
        .bg(move || match tone {
            Tone::Info | Tone::Secure => c::info_bg(),
            Tone::Warning => c::warning_bg(),
            Tone::Danger => c::danger_bg(),
        })
        .child(
            icon(glyph, 16.0)
                .mt(px(1.0))
                .text_color(move || match tone {
                    Tone::Info => c::link(),
                    Tone::Secure => c::primary(),
                    Tone::Warning => c::warning(),
                    Tone::Danger => c::destructive(),
                }),
        )
        .child(
            text(message)
                .flex_1()
                .text_size(px(font::SMALL))
                .line_height(px(18.0))
                .text_color(move || c::foreground()),
        )
}

/// A surface (card) for grouped content that is not a row list.
pub fn card() -> View {
    let sp = spec();
    // Apple and Material: a plain (tonal) surface; Fluent: a layer with
    // its 1px card stroke and a whisper of shadow; Adwaita: flat, a 1px
    // border.
    view()
        .rounded(px(sp.radius.card))
        .bg(move || c::card())
        .when(sp.card_stroke, |v| {
            v.border(if sp.skin == Skin::Fluent {
                px(1.0)
            } else {
                hairline()
            })
            .border_color(move || c::border())
        })
        .when(sp.elevation == Elevation::Shadow, |v| v.shadow_2xs())
        .p_4()
}

// ---------------------------------------------------------------- states

/// An empty state: an icon, a title, one sentence and an optional action.
pub fn empty_state(
    glyph: Icon,
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
    action: Option<AnyElement>,
) -> View {
    // Rhythm: the mark, then the words as one group (title and sentence 6
    // apart), then the action set off by a section gap. It fills an empty screen from its own
    // size (`flex_auto`): with `flex_1`'s zero basis it collapsed in a scrolling page, its words
    // running over what followed (D-0183).
    view()
        .flex_auto()
        .items_center()
        .justify_center()
        .px_8()
        .py_10()
        .child(
            view()
                .size(px(64.0))
                .rounded_full()
                .items_center()
                .justify_center()
                .bg(move || c::accent())
                .text_color(move || c::primary())
                .child(icon(glyph, 28.0)),
        )
        .child(
            view()
                .items_center()
                .mt_4()
                .gap(px(6.0))
                .max_w(px(320.0))
                .child(
                    text(title.into())
                        .text_size(px(font::TITLE))
                        .line_height(px(22.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_center()
                        .role(AccessibilityRole::Header),
                )
                .child(
                    text(message.into())
                        .text_size(px(font::SMALL + 1.0))
                        .line_height(px(20.0))
                        .text_center()
                        .text_color(move || c::muted_fg()),
                ),
        )
        .child(action.map(|a| view().mt_6().child(a)))
}

/// A skeleton bar for content that is loading (no spinners where a shape
/// can show).
pub fn skeleton(width: f32, height: f32) -> View {
    view()
        .w(px(width))
        .h(px(height))
        .rounded(move || radius::sm())
        .bg(move || c::muted())
}

/// A chat-list skeleton row.
pub fn skeleton_row() -> View {
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_3()
        .child(view().size(px(48.0)).rounded_full().bg(move || c::muted()))
        .child(
            view()
                .gap_2()
                .child(skeleton(140.0, 12.0))
                .child(skeleton(220.0, 10.0)),
        )
}
