//! A12: invites, plans and support. Growing and earning without ever
//! feeling pushy: the plans page states exactly what each plan adds, the
//! paywall shows only on this page or when a paid feature is tapped, with
//! "Not now" as visible as the offer, and security is always free.

pub mod catalog;
mod help;
pub(crate) mod invites;
mod standing;

use nsh_core::prelude::*;

use crate::{
    nav,
    store::{Billing, Plan, Store, extras, store},
    ui::*,
};

pub use help::{contact, help, licences, open_article};
pub use invites::{invite_friend, invites};
pub use standing::{standing, standing_banner};

use catalog::{ALWAYS_FREE, PLANS, info, price_label, price_line, provider};

/// The date a paid plan renews or ends. mock: from the store's receipt.
const PERIOD_END_MONTHLY: &str = "17 December 2026";
const PERIOD_END_YEARLY: &str = "17 November 2027";

fn period_end(billing: Billing) -> String {
    // The real backend: the entitlement's last day; the mock otherwise.
    if let Some(day) = extras().with_untracked(|e| e.last_day) {
        return date_of_day(day);
    }
    match billing {
        Billing::Monthly => PERIOD_END_MONTHLY,
        Billing::Yearly => PERIOD_END_YEARLY,
    }
    .to_owned()
}

/// "17 December 2026" for a UTC day number (days since 1970-01-01).
fn date_of_day(day: u32) -> String {
    // Howard Hinnant's civil_from_days.
    let z = i64::from(day) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    format!("{d} {} {y}", MONTHS[usize::try_from(m - 1).unwrap_or(0)])
}

/// A request to show the paywall, from anywhere in the app.
#[derive(Debug, Clone, PartialEq, Default)]
struct PaywallRequest {
    pending: bool,
    feature: SharedString,
}

/// The paywall for a paid `feature` ("Chat themes", "App icons"): opens
/// the plans page with a sheet naming the feature, what Plus includes and
/// its price, and "Not now" (which returns where the user was). Never call
/// it for security or core messaging, and never on a timer.
pub fn paywall(feature: &str) {
    global::<PaywallRequest>().set(PaywallRequest {
        pending: true,
        feature: feature.to_string().into(),
    });
    nav::push("plans", plans_page);
}

/// The plans page, pushed (You → Subscription). Its title is the row's.
pub fn subscription() {
    nav::push("plans", plans_page);
}

fn plans_page() -> Screen {
    // The real entitlement each time the page opens (nothing on the mock).
    store().refresh_plan();
    page("Subscription", plans_body())
}

fn plans_body() -> AnyElement {
    let s = store();
    let billing = signal(extras().with_untracked(|e| e.billing));
    let buying = signal(None::<Plan>);
    let buy_open = signal(false);
    let wall_open = signal(false);
    let wall_feature = signal(SharedString::default());
    let wall_external = stored(false);

    // A paywall asked for from another screen (or this one) opens here.
    let request = global::<PaywallRequest>();
    effect(move || {
        let r = request.get();
        if r.pending {
            untracked(|| {
                wall_feature.set(r.feature.clone());
                wall_external.replace(true);
                wall_open.set(true);
                request.set(PaywallRequest::default());
            });
        }
    });

    let start_purchase = move |plan: Plan| {
        buying.set(Some(plan));
        wall_open.set(false);
        buy_open.set(true);
    };

    let current = move || s.plan();
    let narrow = !wide();

    let content = view()
        .w_full()
        .max_w(px(760.0))
        .self_center()
        .child(current_plan(s))
        .child(section_title("Choose how you pay"))
        .child(view().px_4().child(segmented(
            vec![(Billing::Monthly, "Monthly"), (Billing::Yearly, "Yearly")],
            move || billing.get(),
            move |b| billing.set(b),
        )))
        .child(
            view()
                .px_4()
                .pt_3()
                .gap_3()
                .when(!narrow, |v| v.flex_row().items_stretch())
                .children(PLANS.iter().filter(|p| p.plan != Plan::Free).map(move |p| {
                    plan_card(p.plan, billing, current, start_purchase)
                        .when(!narrow, |v| v.flex_1())
                        .into_element()
                })),
        )
        .child(crate::features::settings::foot(move || {
            price_line(Plan::Plus).map_or_else(String::new, |n| {
                format!("Plus is {n}. Your store may show the price in your currency.")
            })
        }))
        .child(always_free())
        .child(section_title("Manage"))
        .child(group([
            Row::new("Restore purchases")
                .icon(Icon::Refresh)
                .subtitle("Bought Plus on another device? Get it here too.")
                .on_press(move || {
                    if s.restore_purchases() {
                        toast("Your plan is already active on this device");
                    } else {
                        toast("No purchases found for this account");
                    }
                })
                .build(),
            Row::new("Manage subscription")
                .icon(Icon::Settings)
                .subtitle("Change how you pay, or cancel")
                .chevron()
                .on_press(move || nav::push("plans-manage", manage_page))
                .build(),
            Row::new("Family")
                .icon(Icon::Users)
                .subtitle("Invite people to your Family plan")
                .chevron()
                .on_press(move || nav::push("plans-family", family_page))
                .build(),
        ]))
        .child(crate::features::settings::foot(format!(
            "Payments go through the {}. Songbird never sees your card details.",
            provider(os()).name
        )));

    view()
        .flex_1()
        .child(page_scroll(content))
        .child(app_sheet(
            wall_open,
            "Songbird Plus",
            paywall_sheet(
                wall_feature,
                billing,
                wall_open,
                wall_external,
                start_purchase,
            ),
        ))
        .child(app_sheet(
            buy_open,
            provider(os()).name,
            purchase_sheet(s, buying, billing, buy_open),
        ))
        .into_element()
}

/// The card at the top: the plan the user is on.
fn current_plan(s: Store) -> View {
    let ext = extras();
    view().px_4().pt_4().child(
        card()
            .flex_row()
            .items_center()
            .gap_3()
            .child(icon_tile(Icon::Feather))
            .child(
                view()
                    .flex_1()
                    .gap(px(2.0))
                    .child(
                        text(move || format!("You're on {}", info(s.plan()).name))
                            .text_size(px(font::TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .role(AccessibilityRole::Header),
                    )
                    .child(muted(move || {
                        let plan = s.plan();
                        let (billing, cancels) = ext.with(|e| (e.billing, e.cancels_at_period_end));
                        match (plan, cancels) {
                            (Plan::Free, _) => {
                                "Everything you need to talk privately. Plus adds a few extras."
                                    .to_string()
                            }
                            (_, false) => format!(
                                "{} · renews on {}",
                                price_label(plan, billing),
                                period_end(billing)
                            ),
                            (_, true) => format!(
                                "Ends on {}. You won't be charged again.",
                                period_end(billing)
                            ),
                        }
                    })),
            ),
    )
}

/// One plan's card: its name, price, what it adds, and its action.
fn plan_card(
    plan: Plan,
    billing: Signal<Billing>,
    current: impl Fn() -> Plan + Copy + 'static,
    start_purchase: impl Fn(Plan) + Copy + 'static,
) -> View {
    let p = info(plan);
    let is_current = move || current() == plan;
    select_card(is_current)
        .label(move || {
            format!(
                "{}, {}{}",
                p.name,
                price_label(plan, billing.get()),
                if is_current() {
                    ", your current plan"
                } else {
                    ""
                }
            )
        })
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    text(p.name)
                        .flex_1()
                        .text_size(px(font::HEADING))
                        .font_weight(FontWeight::BOLD)
                        .role(AccessibilityRole::Header),
                )
                .child(
                    view()
                        .style_when(move || !is_current(), |st| st.hidden())
                        .child(tag("Current plan")),
                ),
        )
        .child(
            text(move || price_label(plan, billing.get()))
                .text_size(px(font::TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .tabular_nums()
                .text_color(move || c::primary()),
        )
        .child(muted(p.tagline))
        .child(
            view().gap_2().pt_1().pb_2().children(
                p.includes
                    .iter()
                    .map(|f| bullet(Icon::Check, c::primary, *f)),
            ),
        )
        .child(view().flex_grow(1.0))
        .child(view().style_when(is_current, |st| st.hidden()).child(btn(
            format!("Get {}…", p.name),
            if plan == Plan::Plus {
                Variant::Primary
            } else {
                Variant::Outline
            },
            BtnSize::Md,
            move || start_purchase(plan),
        )))
}

/// The calm promise: what never costs anything.
fn always_free() -> View {
    view().px_4().pt_5().child(
        card()
            .gap_2()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(icon(Icon::ShieldCheck, 18.0).text_color(move || c::primary()))
                    .child(
                        text("Security features are always free")
                            .text_size(px(font::body()))
                            .font_weight(FontWeight::SEMIBOLD)
                            .role(AccessibilityRole::Header),
                    ),
            )
            .child(
                view().gap_1().children(
                    ALWAYS_FREE
                        .iter()
                        .map(|f| bullet(Icon::Lock, c::muted_fg, *f)),
                ),
            )
            .child(muted(
                "A paid plan never changes how your messages are protected.",
            )),
    )
}

/// The paywall's sheet body. "Not now" is as large as the offer.
fn paywall_sheet(
    feature: Signal<SharedString>,
    billing: Signal<Billing>,
    open: Signal<bool>,
    external: Stored<bool>,
    start_purchase: impl Fn(Plan) + Copy + 'static,
) -> View {
    let plus = info(Plan::Plus);
    let not_now = move || {
        open.set(false);
        // Opened from another screen: go back there.
        if external.replace(false) {
            nav::pop();
        }
    };
    view()
        .px_4()
        .pb_2()
        .gap_3()
        .child(
            text(move || catalog::paywall_title(&feature.get()))
                .text_size(px(font::HEADING))
                .font_weight(FontWeight::BOLD)
                .role(AccessibilityRole::Header),
        )
        .child(muted("Plus includes, and you can cancel any time:"))
        .child(
            view().gap_2().children(
                plus.includes
                    .iter()
                    .map(|f| bullet(Icon::Check, c::primary, *f)),
            ),
        )
        .child(
            text(move || format!("{}.", price_line(Plan::Plus).unwrap_or_default()))
                .text_size(px(font::SMALL))
                .line_height(px(18.0))
                .text_color(move || c::muted_fg()),
        )
        .child(
            view()
                .flex_row()
                .gap_3()
                .pt_1()
                .child(view().flex_1().child(btn(
                    "Not now",
                    Variant::Outline,
                    BtnSize::Lg,
                    not_now,
                )))
                .child(view().flex_1().child(btn(
                    "Get Plus…",
                    Variant::Primary,
                    BtnSize::Lg,
                    move || {
                        let _ = billing;
                        start_purchase(Plan::Plus)
                    },
                ))),
        )
}

/// Choosing the plan's interval before paying: Subscribe opens the store's own sheet
/// (StoreKit on iOS and macOS, Play Billing on Android) or, on Windows and Linux, the EU
/// provider's checkout in the browser (D-0178).
fn purchase_sheet(
    s: Store,
    buying: Signal<Option<Plan>>,
    billing: Signal<Billing>,
    open: Signal<bool>,
) -> View {
    let ext = extras();
    let busy = move || ext.with(|e| e.purchasing);
    let plan = move || buying.get().unwrap_or(Plan::Plus);
    let prov = provider(os());
    view()
        .px_4()
        .pb_2()
        .gap_3()
        .child(
            card()
                .gap_1()
                .child(
                    text(move || format!("Songbird {}", info(plan()).name))
                        .text_size(px(font::TITLE))
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .child(
                    text(move || price_label(plan(), billing.get()))
                        .text_size(px(font::body()))
                        .tabular_nums(),
                )
                .child(muted(move || {
                    format!("Renews on {} until you cancel.", period_end(billing.get()))
                })),
        )
        .child(segmented(
            vec![(Billing::Monthly, "Monthly"), (Billing::Yearly, "Yearly")],
            move || billing.get(),
            move |b| billing.set(b),
        ))
        .child(muted(format!(
            "{} Prices in your store may vary.",
            prov.manage
        )))
        .child(
            view()
                .flex_row()
                .gap_3()
                .child(view().flex_1().child(btn(
                    "Cancel",
                    Variant::Secondary,
                    BtnSize::Lg,
                    move || open.set(false),
                )))
                .child(
                    view().flex_1().child(
                        btn(
                            move || if busy() { "Confirming…" } else { "Subscribe" },
                            Variant::Primary,
                            BtnSize::Lg,
                            move || {
                                if busy() {
                                    return;
                                }
                                let p = plan();
                                s.purchase(p, billing.get_untracked(), move || {
                                    open.set(false);
                                    toast(format!(
                                        "You're on Songbird {}. Thank you for supporting Songbird.",
                                        info(p).name
                                    ));
                                });
                            },
                        )
                        .disabled(busy),
                    ),
                ),
        )
}

// ---------------------------------------------------------------- manage

/// A row stating a value (not pressable, so no chevron).
pub(crate) fn info_row(title: &'static str, value: impl Into<SharedString>) -> AnyElement {
    let value: SharedString = value.into();
    Row::new(title)
        .trailing(
            text(value)
                .text_size(px(font::SMALL + 1.0))
                .text_color(move || c::muted_fg())
                .number_of_lines(1),
        )
        .build()
}

fn manage_page() -> Screen {
    let s = store();
    let ext = extras();
    let paid = move || s.plan() != Plan::Free;
    let cancels = move || ext.with(|e| e.cancels_at_period_end);
    let prov = provider(os());
    page(
        "Manage subscription",
        page_scroll(
            view()
                .w_full()
                .max_w(px(760.0))
                .self_center()
                .child(when(
                    move || !paid(),
                    || {
                        empty_state(
                            Icon::Feather,
                            "You're on Free",
                            "There's nothing to manage. If you ever subscribe, you can change or cancel it here.",
                            None,
                        )
                    },
                ))
                .child(when(paid, move || {
                    view()
                        .child(section_title("Your plan"))
                        .child(dynamic(move || {
                            let plan = s.plan();
                            let b = ext.with(|e| e.billing);
                            let ends = cancels();
                            untracked(|| {
                                group([
                                    info_row("Plan", info(plan).name),
                                    info_row("Billing", price_label(plan, b)),
                                    info_row(
                                        "Renewal",
                                        if ends { format!("Ends {}", period_end(b)) } else { format!("Renews {}", period_end(b)) },
                                    ),
                                ])
                            })
                        }))
                        .child(crate::features::settings::foot(prov.manage))
                        .child(
                            view()
                                .px_4()
                                .pt_5()
                                .gap_3()
                                .child(when(
                                    move || !cancels(),
                                    move || {
                                        btn("Cancel subscription", Variant::Outline, BtnSize::Md, move || {
                                            let b = extras().with_untracked(|e| e.billing);
                                            let name = info(s.plan()).name;
                                            confirm(
                                                format!("Cancel {name}?"),
                                                format!(
                                                    "{name} stays on until {}. After that you're on Free, with every security feature.",
                                                    period_end(b)
                                                ),
                                                "Cancel subscription",
                                                move || {
                                                    s.cancel_renewal();
                                                    toast(format!("{name} stays on until {}", period_end(b)));
                                                },
                                            );
                                        })
                                    },
                                ))
                                .child(when(cancels, move || {
                                    btn("Keep my subscription", Variant::Primary, BtnSize::Md, move || {
                                        s.resume_renewal();
                                        toast("Your subscription will renew");
                                    })
                                })),
                        )
                }))
                .into_element(),
        ),
    )
}

// ---------------------------------------------------------------- family

/// The Family plan's members: up to six people, each with their own
/// private account.
pub fn family_page() -> Screen {
    let s = store();
    let ext = extras();
    let invite_open = signal(false);
    let name = signal(String::new());
    let is_family = move || s.plan() == Plan::Family;
    let count = move || ext.with(|e| e.family.len()) + 1;
    let me = s.me.get_untracked();
    let body = view()
        .w_full()
        .max_w(px(760.0))
        .self_center()
        .child(when(
            move || !is_family(),
            move || {
                empty_state(
                    Icon::Users,
                    "Share Plus with your family",
                    "With the Family plan, up to 6 people get Plus. Everyone keeps their own private account, and nobody can read anyone else's messages.",
                    Some(btn("See the Family plan", Variant::Primary, BtnSize::Md, nav::pop).into_element()),
                )
                .into_element()
            },
        ))
        // A member of someone else's family (real backend): join with their link, or leave.
        .child(when(
            move || !is_family() && crate::backend::is_live() && crate::backend::registered(),
            move || {
                let link = signal(String::new());
                let member = signal(crate::backend::plans::has_seat());
                view()
                    .child(section_title("In someone's family?"))
                    .child(when(
                        move || !member.get(),
                        move || {
                            view()
                                .px_4()
                                .gap_3()
                                .child(labelled_field(
                                    "Family link",
                                    field("https://songbird.chat/family#…", link),
                                    "The link the family's manager sent you. Nobody else learns you joined.",
                                    || false,
                                ))
                                .child(view().flex_row().child(
                                    btn("Join", Variant::Primary, BtnSize::Md, move || {
                                        s.join_family(&link.get_untracked());
                                        member.set(true);
                                        toast("Joined. Plus arrives in a moment.");
                                    })
                                    .disabled(move || link.with(|l| l.trim().is_empty())),
                                ))
                        },
                    ))
                    .child(when(
                        move || member.get(),
                        move || {
                            view().px_4().flex_row().child(btn(
                                "Leave the family",
                                Variant::Secondary,
                                BtnSize::Md,
                                move || {
                                    confirm(
                                        "Leave the family?".to_string(),
                                        "You keep Plus until the end of this month, then you're on Free.",
                                        "Leave",
                                        move || {
                                            s.leave_family();
                                            member.set(false);
                                            toast("You left the family");
                                        },
                                    );
                                },
                            ))
                        },
                    ))
            },
        ))
        .child(when(is_family, move || {
            let me = me.clone();
            view()
                .child(section_title(move || format!("{} of 6 people", count())))
                .child(
                    view()
                        .mx_4()
                        .rounded(move || radius::lg())
                        .overflow_hidden()
                        .bg(move || c::card())
                        .border_1()
                        .border_color(move || c::border())
                        .child(
                            Row::new(format!("{} (you)", me.name))
                                .leading(avatar(me.name.clone(), me.seed, 36.0))
                                .subtitle("Manager")
                                .build(),
                        )
                        .child(each(
                            move || ext.with(|e| e.family.clone()),
                            |m| m.id,
                            move |m, _| {
                                let m = m.get_untracked();
                                let id = m.id;
                                let who = m.name.clone();
                                let link = m.link.clone();
                                let copy_label = format!("Copy {who}'s link");
                                view().child(separator_inset(56.0)).child(
                                    Row::new(m.name.clone())
                                        .leading(avatar(m.name.clone(), id, 36.0))
                                        .subtitle(if m.pending { "Invited · hasn't joined yet" } else { "Member" })
                                        .trailing(view().flex_row().items_center().gap_1()
                                        // The seat's link holds its secret: shared by the owner, by hand.
                                        .when(link.is_some(), move |v| {
                                            let link = link.clone().unwrap_or_default();
                                            v.child(icon_btn(Icon::Copy, copy_label.clone(), move || {
                                                set_clipboard(link.clone());
                                                toast("Link copied. Send it to them yourself.");
                                            })
                                            .text_color(move || c::muted_fg()))
                                        })
                                        .child(icon_btn(Icon::Close, format!("Remove {who}"), move || {
                                            let who = who.clone();
                                            confirm(
                                                format!("Remove {who}?"),
                                                "They go back to Free at the end of this period. Their account and chats stay theirs.",
                                                "Remove",
                                                move || {
                                                    s.family_remove(id);
                                                    toast(format!("{who} removed from your family"));
                                                },
                                            );
                                        })
                                        .text_color(move || c::muted_fg())))
                                        .build(),
                                )
                            },
                        )),
                )
                .child(crate::features::settings::foot("Family members can't see each other's chats, contacts or activity. You only see who is in the family."))
                .child(
                    view().px_4().pt_4().child(
                        btn_with_icon("Invite someone…", Some(Icon::UserPlus), Variant::Primary, BtnSize::Md, move || {
                            name.set(String::new());
                            invite_open.set(true);
                        })
                        .disabled(move || count() >= 6),
                    ),
                )
        }));
    page(
        "Family",
        view().flex_1().child(page_scroll(body)).child(app_sheet(
            invite_open,
            "Invite to Family",
            view()
                .px_4()
                .pb_2()
                .gap_3()
                .child(labelled_field(
                    "Name",
                    field("Their name", name),
                    "They get a link to join. You can remove them at any time.",
                    || false,
                ))
                .child(
                    btn("Send invite", Variant::Primary, BtnSize::Lg, move || {
                        let n = name.get_untracked().trim().to_string();
                        if n.is_empty() {
                            return;
                        }
                        s.family_invite(n.clone());
                        invite_open.set(false);
                        toast(if crate::backend::is_live() {
                            format!("{n}'s seat is ready. Copy its link and send it to them.")
                        } else {
                            format!("Invite sent to {n}")
                        });
                    })
                    .disabled(move || name.with(|n| n.trim().is_empty())),
                ),
        )),
    )
}

// ---------------------------------------------------------------- announcement

/// The one announcement showing now. mock: from the server's signed
/// notices (A9), at most one at a time.
const ANNOUNCEMENT: (&str, &str, &str) = (
    "desktop-2026-11",
    "Songbird for desktop is here",
    "Link your computer from You → Linked devices. Your messages stay end\u{2011}to\u{2011}end encrypted on every device.",
);

/// A single dismissible card for important news. Place it at the top of
/// a list (never inside a conversation); once dismissed it never returns.
pub fn announcement_card() -> AnyElement {
    let s = store();
    let (id, title, message) = ANNOUNCEMENT;
    let gone = signal(s.announcement_dismissed(id));
    let leaving = signal(false);
    let reduced = motion::reduced();
    when(
        move || !gone.get(),
        move || {
            view()
                .px_4()
                .py_2()
                .opacity(move || if leaving.get() { 0.0_f32 } else { 1.0 })
                .translate_y(move || {
                    if leaving.get() && !reduced {
                        px(-4.0)
                    } else {
                        px(0.0)
                    }
                })
                .transition_all()
                .duration(motion::FAST)
                .ease_in()
                .child(
                    card()
                        .flex_row()
                        .items_start()
                        .gap_3()
                        .role(AccessibilityRole::Summary)
                        .child(icon_tile(Icon::Megaphone))
                        .child(
                            view()
                                .flex_1()
                                .gap(px(2.0))
                                .child(
                                    text(title)
                                        .text_size(px(font::body()))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .role(AccessibilityRole::Header),
                                )
                                .child(muted(message)),
                        )
                        .child(icon_btn(Icon::Close, "Dismiss", move || {
                            s.dismiss_announcement(id);
                            leaving.set(true);
                            set_timeout(
                                std::time::Duration::from_millis(motion::FAST as u64),
                                move || gone.set(true),
                            );
                        })),
                )
        },
    )
    .into_element()
}

#[cfg(test)]
mod date_tests {
    #[test]
    fn days_become_dates() {
        assert_eq!(super::date_of_day(0), "1 January 1970");
        assert_eq!(super::date_of_day(20_774), "17 November 2026");
        assert_eq!(super::date_of_day(20_804), "17 December 2026");
    }
}
