//! A11 backups and transfer: the backup settings, the set-up wizard,
//! restoring and moving history to a new phone. People should actually
//! set backups up, and actually be able to restore.

mod restore;
mod transfer;
mod wizard;

use std::rc::Rc;

use nsh_core::prelude::*;

use crate::{
    nav,
    store::{
        BackupDestination, Store,
        ext_onboarding::{BackupProblem, Frequency},
        store,
    },
    ui::*,
};

pub(crate) use restore::restore_view;
pub(crate) use wizard::wizard_body;

/// How a guided flow shown inside another one (the backup wizard and the
/// restore inside onboarding) shares the host's single Back, so the
/// screen never shows two.
#[derive(Clone, Copy)]
pub(crate) struct FlowHost {
    /// Set by the inner flow: steps back inside it, `false` at its start
    /// (the host then leaves it).
    pub back: Stored<Option<Rc<dyn Fn() -> bool>>>,
    /// Set by the inner flow while Back would make no sense (a restore
    /// running or finished, backups just turned on).
    pub hide_back: Signal<bool>,
}

impl FlowHost {
    pub(crate) fn new() -> Self {
        Self {
            back: stored(None),
            hide_back: signal(false),
        }
    }

    /// Forgets the inner flow (the host moved to another step).
    pub(crate) fn reset(&self) {
        self.back.replace(None);
        self.hide_back.set(false);
    }
}

/// The backup settings, pushed (linked from You → Chats → Backups).
pub fn backup_settings() {
    nav::push("backups", || page("Backups", settings_body()));
}

/// The set-up wizard, pushed: why → where → recovery key → how often → done.
pub fn backup_wizard() {
    nav::push("backup-wizard", || {
        page(
            "Set up backups",
            wizard_body(false, Rc::new(nav::pop), None),
        )
    });
}

/// Restoring from the backup, pushed (from the backup settings).
pub fn restore() {
    nav::push("backup-restore", || {
        page(
            "Restore from a backup",
            restore_view(false, Rc::new(nav::pop), None, None),
        )
    });
}

/// "Transfer chat history to a new phone", pushed.
pub fn transfer() {
    nav::push("transfer", || {
        page("Transfer to a new phone", transfer::transfer_body())
    });
}

// ---------------------------------------------------------------- destinations

/// The destinations this platform offers, in the order shown.
pub(crate) fn destinations() -> Vec<BackupDestination> {
    use BackupDestination::*;
    let apple = matches!(
        os(),
        nsh_core::platform::Os::Ios | nsh_core::platform::Os::MacOs
    );
    let mut all = vec![SongbirdServers, EncryptedFile];
    if apple {
        all.push(ICloudDrive);
    }
    all.extend([GoogleDrive, OneDrive, Dropbox, Nextcloud, S3]);
    all
}

pub(crate) fn dest_icon(d: BackupDestination) -> Icon {
    use BackupDestination::*;
    match d {
        SongbirdServers => Icon::Feather,
        EncryptedFile => Icon::File,
        // Distinct generic glyphs, never the providers' logos.
        ICloudDrive => Icon::Cloud,
        GoogleDrive => Icon::HardDrive,
        OneDrive => Icon::Inbox,
        Dropbox => Icon::Archive,
        Nextcloud => Icon::Server,
        S3 => Icon::Database,
    }
}

pub(crate) fn dest_note(d: BackupDestination) -> &'static str {
    use BackupDestination::*;
    if !crate::backend::backup::available(d) {
        return crate::backend::backup::unavailable_reason(d);
    }
    match d {
        SongbirdServers => "Kept by Songbird, which can't open it.",
        EncryptedFile => "A file you keep yourself, on a drive or a computer.",
        ICloudDrive => "Your iCloud Drive.",
        GoogleDrive => "Your Google Drive.",
        OneDrive => "Your OneDrive.",
        Dropbox => "Your Dropbox.",
        Nextcloud => "Your own server, over WebDAV.",
        S3 => "Any S3-compatible bucket.",
    }
}

/// The destination rows, one chosen (a check mark and "Selected" — not
/// colour alone).
pub(crate) fn dest_group(
    options: Vec<BackupDestination>,
    chosen: impl Fn() -> Option<BackupDestination> + Copy + 'static,
    on_pick: impl Fn(BackupDestination) + Copy + 'static,
) -> View {
    group(options.into_iter().map(move |d| {
        let selected = move || chosen() == Some(d);
        Row::new(d.label())
            .icon(dest_icon(d))
            .subtitle(dest_note(d))
            .trailing(
                view()
                    .size(px(22.0))
                    .rounded_full()
                    .items_center()
                    .justify_center()
                    .border_2()
                    .border_color(move || if selected() { c::primary() } else { c::input() })
                    .bg(move || {
                        if selected() {
                            c::primary()
                        } else {
                            AdaptiveColor::TRANSPARENT
                        }
                    })
                    .text_color(move || c::on_primary())
                    .transition_colors()
                    .duration(motion::FAST)
                    .label(move || if selected() { "Selected" } else { "" })
                    .child(icon(Icon::Check, 14.0).style_when(move || !selected(), |s| s.hidden())),
            )
            .on_press(move || on_pick(d))
            .build()
    }))
}

type Extra = crate::store::ext_onboarding::BackupExtra;

/// A field over one text of the backup state, written on each change.
fn extra_field(
    s: Store,
    placeholder: &'static str,
    get: fn(&Extra) -> String,
    set: fn(&mut Extra, String),
) -> nsh_core::element::Input {
    let extra = s.backup_extra();
    let value = signal(extra.with_untracked(get));
    field(placeholder, value)
        .on_change(move |t: &str| {
            value.set(t.to_owned());
            extra.update(|b| set(b, t.to_owned()));
        })
        .auto_capitalize(AutoCapitalize::None)
        .auto_correct(false)
}

/// The fields a self-hosted destination needs.
pub(crate) fn dest_fields(s: Store, d: BackupDestination) -> Option<View> {
    match d {
        BackupDestination::Nextcloud => Some(
            card()
                .gap_3()
                .child(labelled_field(
                    "Server URL",
                    extra_field(
                        s,
                        "https://cloud.example.org",
                        |b| b.nextcloud_url.clone(),
                        |b, v| b.nextcloud_url = v,
                    )
                    .keyboard_type(KeyboardType::Url),
                    "Your Nextcloud's address. Backups go in a Songbird folder there.",
                    || false,
                ))
                .child(labelled_field(
                    "Username",
                    extra_field(
                        s,
                        "Nextcloud username",
                        |b| b.nextcloud_user.clone(),
                        |b, v| b.nextcloud_user = v,
                    ),
                    "",
                    || false,
                ))
                .child(labelled_field(
                    "App password",
                    extra_field(
                        s,
                        "App password",
                        |b| b.nextcloud_password.clone(),
                        |b, v| b.nextcloud_password = v,
                    )
                    .masked(true),
                    "Make one in Nextcloud under Settings → Security. It stays on this device.",
                    || false,
                )),
        ),
        BackupDestination::S3 => Some(
            card()
                .gap_3()
                .child(labelled_field(
                    "Endpoint",
                    extra_field(
                        s,
                        "https://s3.example.com",
                        |b| b.s3_endpoint.clone(),
                        |b, v| b.s3_endpoint = v,
                    )
                    .keyboard_type(KeyboardType::Url),
                    "",
                    || false,
                ))
                .child(labelled_field(
                    "Bucket",
                    extra_field(
                        s,
                        "my-songbird-backups",
                        |b| b.s3_bucket.clone(),
                        |b, v| b.s3_bucket = v,
                    ),
                    "",
                    || false,
                ))
                .child(labelled_field(
                    "Access key ID",
                    extra_field(
                        s,
                        "Access key ID",
                        |b| b.s3_key.clone(),
                        |b, v| b.s3_key = v,
                    ),
                    "",
                    || false,
                ))
                .child(labelled_field(
                    "Secret access key",
                    extra_field(
                        s,
                        "Secret access key",
                        |b| b.s3_secret.clone(),
                        |b, v| b.s3_secret = v,
                    )
                    .masked(true),
                    "It stays on this device. Give the key access to this bucket only.",
                    || false,
                ))
                .child(labelled_field(
                    "Region",
                    extra_field(
                        s,
                        "us-east-1",
                        |b| b.s3_region.clone(),
                        |b, v| b.s3_region = v,
                    ),
                    "Leave empty unless your provider needs one.",
                    || false,
                )),
        ),
        _ => None,
    }
}

/// Whether the fields `d` needs are filled.
pub(crate) fn dest_ready(s: Store, d: BackupDestination) -> bool {
    s.backup_extra().with(|b| match d {
        BackupDestination::Nextcloud => {
            b.nextcloud_url.starts_with("https://")
                && !b.nextcloud_user.trim().is_empty()
                && !b.nextcloud_password.is_empty()
        }
        BackupDestination::S3 => {
            b.s3_endpoint.starts_with("https://")
                && !b.s3_bucket.trim().is_empty()
                && !b.s3_key.trim().is_empty()
                && !b.s3_secret.is_empty()
        }
        d => crate::backend::backup::available(d),
    })
}

// ---------------------------------------------------------------- settings page

/// The note under the backup settings, in the platform's terms.
fn os_note() -> &'static str {
    use nsh_core::platform::Os;
    match os() {
        Os::Ios => {
            "Your chats stay out of iCloud's own backups. Songbird backups are encrypted with your recovery key."
        }
        Os::Android => {
            "Your chats stay out of Android's own backups. Songbird backups are encrypted with your recovery key."
        }
        _ => "Backups are encrypted with your recovery key before they leave this computer.",
    }
}

fn settings_body() -> AnyElement {
    let s = store();
    let on = move || s.settings.with(|x| x.backup_on);
    page_scroll(
        view()
            .w_full()
            .max_w(px(760.0))
            .self_center()
            .pt_2()
            .child(when(on, move || backups_on(s)).otherwise(move || backups_off(s)))
            .child(crate::features::settings::foot(os_note()).pb_4()),
    )
    .into_element()
}

fn backups_off(s: Store) -> View {
    view()
        .child(
            card()
                .mx_4()
                .mt_2()
                .items_center()
                .gap_3()
                .py_6()
                .child(hero_icon(Icon::Cloud))
                .child(
                    text("Back up your chats")
                        .text_size(px(font::TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .role(AccessibilityRole::Header),
                )
                .child(
                    muted(if is_desktop() {
                        "Keep your messages if you lose or replace a device. Your recovery key locks them, so only you can open them."
                    } else {
                        "Keep your messages if you lose or replace your phone. Backups are encrypted with your recovery key, so only you can open them."
                    })
                        .text_center()
                        .max_w(px(380.0)),
                )
                .child(btn("Set up backups", Variant::Primary, BtnSize::Md, backup_wizard).mt_1()),
        )
        .child(section_title("More"))
        .child(more_rows(s, false))
}

fn more_rows(s: Store, on: bool) -> View {
    let mut rows = Vec::new();
    // Phone to phone only: a computer links to the account instead.
    if !is_desktop() {
        rows.push(
            Row::new("Transfer to a new phone")
                .icon(Icon::Smartphone)
                .subtitle("Straight to a new phone, over Wi\u{2011}Fi")
                .chevron()
                .on_press(transfer)
                .build(),
        );
    }
    rows.push(
        Row::new("Restore from a backup")
            .icon(Icon::Download)
            .chevron()
            .on_press(restore)
            .build(),
    );
    if on {
        rows.push(
            Row::new("Turn off backups")
                .icon(Icon::Trash)
                .danger()
                .on_press(move || {
                    let dest = s.settings.with_untracked(|x| x.backup_destination.label());
                    confirm(
                        "Turn off backups?",
                        format!(
                            "Your backup in {dest} is deleted, and new messages aren't backed up."
                        ),
                        "Turn off",
                        move || {
                            s.disable_backups();
                            toast("Backups are off");
                        },
                    )
                })
                .build(),
        );
    }
    group(rows)
}

fn problem_text(p: BackupProblem, dest: &str, note: Option<String>) -> String {
    match p {
        BackupProblem::Other => format!(
            "Your last backup didn't finish. {}",
            note.unwrap_or_else(|| "Try again.".into())
        ),
        BackupProblem::StorageFull => {
            format!(
                "Your last backup didn't finish because {dest} is full. Free up space there, or choose another place."
            )
        }
        BackupProblem::SignedOut => {
            format!(
                "Your last backup didn't finish because Songbird was signed out of {dest}. Sign in again to keep backing up."
            )
        }
    }
}

fn backups_on(s: Store) -> View {
    let extra = s.backup_extra();
    let choose_open = signal(false);
    let second_open = signal(false);
    let dest = move || s.settings.with(|x| x.backup_destination);
    let running = move || extra.with(|b| b.running);
    let problem = move || extra.with(|b| b.problem);
    let freq = move || extra.with(|b| b.frequency);

    let status = card()
        .mx_4()
        .mt_2()
        .gap_3()
        .child(
            view()
                .flex_row()
                .items_start()
                .gap_3()
                .child(view().child(dynamic(move || icon_tile(dest_icon(dest())).into_element())))
                .child(
                    view()
                        .flex_1()
                        .gap(px(2.0))
                        .child(
                            text(move || format!("Backing up to {}", dest().label()))
                                .text_size(px(font::body()))
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(muted(move || {
                            let last = s.settings.with(|x| x.backup_last.clone());
                            let size = s.settings.with(|x| x.backup_size.clone());
                            match last {
                                Some(at) => format!("Last backup {} · {size}", at.to_lowercase()),
                                None => "No backup yet".into(),
                            }
                        }))
                        .child(muted(move || {
                            let media = s.settings.with(|x| x.backup_include_media);
                            let second = extra
                                .with(|b| b.second)
                                .map(|d| format!(" · also {}", d.label()))
                                .unwrap_or_default();
                            format!(
                                "{} · {}{second}",
                                freq().label(),
                                if media {
                                    "Photos and videos included"
                                } else {
                                    "Messages only"
                                }
                            )
                        })),
                ),
        )
        .child(when(
            move || running().is_some(),
            move || {
                view()
                    .gap_2()
                    .child(progress_bar("Backing up", move || running().unwrap_or(0.0)))
                    .child(muted(move || {
                        format!(
                            "Backing up… {}%",
                            (running().unwrap_or(0.0) * 100.0).round()
                        )
                    }))
            },
        ))
        .child(when(
            move || problem().is_some(),
            move || {
                view()
                    .gap_2()
                    .child(banner(Tone::Warning, Icon::Alert, move || {
                        problem()
                            .map(|p| {
                                problem_text(p, dest().label(), extra.with(|b| b.note.clone()))
                            })
                            .unwrap_or_default()
                    }))
                    .child(view().flex_row().child(btn(
                        move || match problem() {
                            Some(BackupProblem::SignedOut) => "Sign in again",
                            Some(BackupProblem::Other) => "Try again",
                            _ => "Fix",
                        },
                        Variant::Primary,
                        BtnSize::Sm,
                        move || match problem() {
                            Some(BackupProblem::StorageFull) => choose_open.set(true),
                            Some(BackupProblem::Other) => s.back_up_now(),
                            // Signed out of the destination: choose it again (its sign-in
                            // happens there). Only the demo says it signed in by itself.
                            _ if !crate::store::demo() => choose_open.set(true),
                            _ => {
                                s.back_up_now();
                                toast(format!("Signed in to {}", dest().label()));
                            }
                        },
                    )))
            },
        ))
        .child(
            view().flex_row().child(
                btn_with_icon(
                    move || {
                        if running().is_some() {
                            "Backing up…"
                        } else {
                            "Back up now"
                        }
                    },
                    Some(Icon::Upload),
                    Variant::Secondary,
                    BtnSize::Md,
                    move || s.back_up_now(),
                )
                .disabled(move || running().is_some()),
            ),
        );

    let others: Vec<BackupDestination> = destinations();
    view()
        .child(status)
        .child(section_title("Back up to"))
        .child(dest_group(others.clone(), move || Some(dest()), move |d| {
            s.settings_update(|x| x.backup_destination = d);
            extra.update(|b| {
                if b.second == Some(d) {
                    b.second = None;
                }
                b.problem = None;
            });
        }))
        .child(dynamic(move || {
            let d = dest();
            untracked(move || match dest_fields(s, d) {
                Some(f) => f.mx_4().mt_3().into_element(),
                None => view().into_element(),
            })
        }))
        .child(
            group([Row::new("Second destination")
                .icon(Icon::Layers)
                .subtitle("Keep a second copy somewhere else")
                .trailing(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(
                            text(move || extra.with(|b| b.second.map(|d| d.label()).unwrap_or("None")))
                                .text_size(px(font::SMALL + 1.0))
                                .text_color(move || c::muted_fg()),
                        )
                        .child(icon(Icon::ChevronRight, 16.0).text_color(move || c::muted_fg())),
                )
                .on_press(move || second_open.set(true))
                .build()])
            .mt_3(),
        )
        .child(section_title("Options"))
        .child(group([
            toggle_row(
                "Include photos and videos",
                Some("Backups are bigger with media."),
                move || s.settings.with(|x| x.backup_include_media),
                move |v| s.settings_update(|x| x.backup_include_media = v),
            ),
            view()
                .px_4()
                .py_3()
                .gap_2()
                .child(text("How often").text_size(px(font::body())))
                .child(segmented(
                    vec![(Frequency::Daily, "Daily"), (Frequency::Weekly, "Weekly"), (Frequency::Monthly, "Monthly")],
                    freq,
                    move |f| s.set_backup_frequency(f),
                ))
                .into_element(),
        ]))
        .child(section_title("More"))
        .child(more_rows(s, true))
        .child(demo_rows(s))
        .child(app_sheet(
            choose_open,
            "Back up to",
            view().pb_2().child(dest_group(others.clone(), move || Some(dest()), move |d| {
                s.settings_update(|x| x.backup_destination = d);
                choose_open.set(false);
                if dest_ready(s, d) {
                    s.back_up_now();
                }
            })),
        ))
        .child(app_sheet(
            second_open,
            "Second destination",
            view()
                .pb_2()
                .gap_3()
                .child(
                    muted("A second copy keeps your chats safe if one place has a problem. Both are encrypted the same way.")
                        .px_4(),
                )
                .child(group([Row::new("None")
                    .icon(Icon::Close)
                    .on_press(move || {
                        extra.update(|b| b.second = None);
                        second_open.set(false);
                    })
                    .build()]))
                .child(dest_group(
                    others.into_iter().filter(|d| *d != BackupDestination::Nextcloud && *d != BackupDestination::S3).collect(),
                    move || extra.with(|b| b.second),
                    move |d| {
                        if d == dest() {
                            toast("That's already your main destination");
                            return;
                        }
                        extra.update(|b| b.second = Some(d));
                        second_open.set(false);
                    },
                )),
        ))
}

/// Demo states, for reviewing the error look (debug builds only).
fn demo_rows(s: Store) -> Option<View> {
    if !cfg!(debug_assertions) {
        return None;
    }
    Some(
        view()
            .child(section_title("Preview states (debug builds)"))
            .child(group([
                Row::new("Show a full-storage problem")
                    .icon(Icon::Alert)
                    .on_press(move || s.simulate_backup_problem(BackupProblem::StorageFull))
                    .build(),
                Row::new("Show a signed-out problem")
                    .icon(Icon::Alert)
                    .on_press(move || s.simulate_backup_problem(BackupProblem::SignedOut))
                    .build(),
            ])),
    )
}
