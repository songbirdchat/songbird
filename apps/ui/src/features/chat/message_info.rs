//! Message info, pushed: the message as it shows in the chat, then who
//! read it and who it reached (per member in groups, with times), or
//! when it was delivered and read in a direct chat.

use nsh_core::prelude::*;

use super::{
    Conv, bubbles,
    list::{Item, ItemMemo},
    logic::Place,
};
use crate::{
    nav,
    store::{ChatId, Minutes, MsgId, clock, day_label, store},
    ui::*,
};

fn at(m: Minutes) -> String {
    format!("{}, {}", day_label(m), clock(m))
}

/// The message info page for message `id` of `chat`.
pub fn message_info(chat: ChatId, id: MsgId) {
    nav::push(format!("message-info:{id}"), move || {
        page("Message info", body(chat, id))
    });
}

fn body(chat: ChatId, id: MsgId) -> AnyElement {
    let s = store();
    let Some(m) = s.message(chat, id) else {
        return empty_state(
            Icon::Info,
            "Message not found",
            "It may have been deleted.",
            None,
        )
        .into_element();
    };
    let cv = Conv::new(chat, s);
    let item: ItemMemo = computed(move || {
        s.message(chat, id).map(|m| Item {
            m,
            place: Place {
                day_break: false,
                time: false,
                first: true,
                last: true,
            },
        })
    });
    // The message on the chat's own canvas (its theme), a hairline under
    // the band (it is close to the page in dark); the bubble's edge on the
    // cards' edge.
    let preview = view()
        .bg(move || cv.canvas())
        .border_b(hairline())
        .border_color(move || c::border())
        .py_6()
        .items_end()
        .child(
            view()
                .w_full()
                .max_w(px(640.0))
                .px_4()
                .self_center()
                .items_end()
                .child(bubbles::bubble_column(cv, item, false)),
        );
    // The tick in an icon tile's width, so the labels sit on the same
    // column as every other card's titles (and its separators).
    let mark = |tick: Tick| view().w(px(30.0)).items_center().child(ticks(move || tick));
    let sent_row = move || {
        Row::new("Sent")
            .leading(mark(Tick::Sent))
            .trailing(muted(at(m.at)))
            .build()
    };

    // A real message's receipts as app-core keeps them, each level's own time (D-0184).
    let real = crate::backend::chat::receipt_times(id);
    let when = |t: Option<Option<Minutes>>| match t {
        Some(Some(t)) => Some(at(t)),
        Some(None) => Some("Earlier".to_string()),
        None => None,
    };
    let group_receipts: Vec<(u64, Option<Minutes>, Option<Minutes>)> = match &real {
        Some(r) if cv.is_group() => r
            .iter()
            .map(|(who, d, rd, _)| (*who, d.map(|t| t.unwrap_or(m.at)), rd.map(|t| t.unwrap_or(m.at))))
            .collect(),
        _ => m.receipts.clone(),
    };
    let receipts = if cv.is_group() && !group_receipts.is_empty() {
        let read: Vec<_> = group_receipts
            .iter()
            .filter_map(|(who, _, r)| r.map(|r| (*who, r)))
            .collect();
        let delivered: Vec<_> = group_receipts
            .iter()
            .filter(|(_, _, r)| r.is_none())
            .filter_map(|(who, d, _)| d.map(|d| (*who, d)))
            .collect();
        let waiting: Vec<_> = group_receipts
            .iter()
            .filter(|(_, d, r)| d.is_none() && r.is_none())
            .map(|(w, _, _)| *w)
            .collect();
        let people = move |list: Vec<(u64, Minutes)>, empty: &'static str| {
            if list.is_empty() {
                return footnote(empty).into_element();
            }
            group(list.into_iter().map(move |(who, when)| {
                let name = s.contact_name(Some(who));
                Row::new(name.clone())
                    .leading(avatar(name, who, 36.0))
                    .trailing(muted(at(when)))
                    .build()
            }))
            .into_element()
        };
        view()
            .child(view().pt_4().child(group([sent_row()])))
            .child(section_title("Read by"))
            .child(people(read, "Nobody has read it yet."))
            .child(section_title("Delivered to"))
            .child(people(delivered, "Everyone who received it has read it."))
            .child((!waiting.is_empty()).then(|| {
                view()
                    .child(section_title("Not delivered yet"))
                    .child(group(waiting.into_iter().map(move |who| {
                        let name = s.contact_name(Some(who));
                        Row::new(name.clone())
                            .leading(avatar(name, who, 36.0))
                            .subtitle("Their phone may be off")
                            .build()
                    })))
            }))
            .into_element()
    } else {
        let status = m.status;
        let (delivered, read, played) = match real.as_ref().and_then(|r| r.first()) {
            // A real chat: the times its receipts arrived.
            Some((_, d, rd, p)) => (when(*d), when(*rd), when(*p)),
            // A real message nobody answered yet.
            None if real.is_some() => (None, None, None),
            // The demo's: made from the tick, a minute apart.
            None => (
                (status >= Tick::Delivered && status != Tick::Failed).then(|| at(m.at + 1)),
                (status >= Tick::Read && status != Tick::Failed).then(|| at(m.at + 2)),
                (status == Tick::Played).then(|| at(m.at + 3)),
            ),
        };
        let row = move |label: &'static str, tick: Tick, when: Option<String>| {
            Row::new(label)
                .leading(mark(tick))
                .trailing(muted(when.unwrap_or_else(|| "Not yet".into())))
                .build()
        };
        let mut rows = vec![
            row("Read", Tick::Read, read),
            row("Delivered", Tick::Delivered, delivered),
            sent_row(),
        ];
        if played.is_some() {
            rows.insert(0, row("Played", Tick::Played, played));
        }
        if status == Tick::Failed {
            rows = vec![
                Row::new("Not sent")
                    .leading(mark(Tick::Failed))
                    .subtitle("Tap the message to try again")
                    .build(),
            ];
        }
        view().pt_4().child(group(rows)).into_element()
    };

    page_scroll(
        view()
            .child(preview)
            // One readable column, the footnote on the cards' edge.
            .child(
                view()
                    .w_full()
                    .max_w(px(640.0))
                    .self_center()
                    .child(receipts)
                    .child(view().h(px(24.0))),
            ),
    )
    .into_element()
}
