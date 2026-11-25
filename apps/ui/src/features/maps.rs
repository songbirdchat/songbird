//! Maps (D-0142): OpenStreetMap vector tiles from Songbird's own server
//! (`GET /v1/maps/v1/<z>/<x>/<y>`, no account named), decoded and styled by
//! app-core (`songbird_app_core::maps`) and drawn here on a canvas. No map
//! company is asked anything.
//!
//! - [`map`]: a map at a centre and zoom; interactive (pan, zoom buttons)
//!   or a still preview (a location bubble).
//! - [`pick`]: the location picker: the user moves the map under a fixed pin
//!   and sends that place.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use nsh_core::prelude::*;
use songbird_app_core::maps::{self, Class, Shape};

use crate::ui::*;

/// Tile size in points.
const TILE: f64 = 256.0;

#[derive(Clone)]
enum Slot {
    Loading,
    Ready(Rc<Vec<Shape>>),
    Missing,
}

thread_local! {
    static TILES: RefCell<HashMap<(u8, u32, u32), Slot>> = RefCell::new(HashMap::new());
}

/// Bumps when a tile arrives, so maps draw again.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Revision(u64);

fn revision() -> Signal<Revision> {
    global::<Revision>()
}

fn tile(z: u8, x: u32, y: u32) -> Option<Rc<Vec<Shape>>> {
    let slot = TILES.with_borrow(|t| t.get(&(z, x, y)).cloned());
    match slot {
        Some(Slot::Ready(s)) => Some(s),
        Some(Slot::Loading | Slot::Missing) => None,
        None => {
            TILES.with_borrow_mut(|t| t.insert((z, x, y), Slot::Loading));
            Owner::detached().run(|| {
                spawn(async move {
                    let got =
                        crate::backend::unsigned("GET", &format!("/v1/maps/v1/{z}/{x}/{y}")).await;
                    let slot = match got {
                        Ok((200, body)) => maps::shapes(&body, z)
                            .map_or(Slot::Missing, |s| Slot::Ready(Rc::new(s))),
                        Ok((204, _)) => Slot::Ready(Rc::new(Vec::new())),
                        _ => Slot::Missing,
                    };
                    TILES.with_borrow_mut(|t| {
                        // Keep the cache bounded: drop everything past 512 tiles.
                        if t.len() > 512 {
                            t.clear();
                        }
                        t.insert((z, x, y), slot);
                    });
                    revision().update(|r| r.0 += 1);
                })
            });
            None
        }
    }
}

fn color(class: Class, dark: bool) -> Rgba {
    let (light, night) = match class {
        Class::Water | Class::Waterway => (0xA9C8E8, 0x22374D),
        Class::Green => (0xCFE5C2, 0x1F3324),
        Class::Urban => (0xEDE7DF, 0x2A2A2E),
        Class::Building => (0xDCD5CC, 0x34343A),
        Class::Motorway => (0xF2B36B, 0x9C6B2E),
        Class::Primary => (0xF7D58C, 0x8A7440),
        Class::Secondary | Class::Minor | Class::Path => (0xFFFFFF, 0x55555D),
        Class::Rail => (0xB4B0AA, 0x6C6C72),
        Class::Boundary => (0xA58FB5, 0x7E6A8E),
        Class::Place => (0x3A3540, 0xE6E1EA),
    };
    rgb(if dark { night } else { light })
}

fn paint(center: (f64, f64), zoom: f64, size: (f64, f64), dark: bool) -> Painter {
    let (w, h) = size;
    let mut p = Painter::new()
        .fill_style(rgb(if dark { 0x1B1B1F } else { 0xF3EFE8 }))
        .fill_rect(0.0, 0.0, w as f32, h as f32);
    if w < 1.0 || h < 1.0 {
        return p;
    }
    let iz = zoom.floor().clamp(0.0, f64::from(maps::MAX_ZOOM));
    let z = iz as u8;
    let scale = TILE * 2f64.powf(zoom - iz);
    let (cx, cy) = maps::project(center.0, center.1, iz);
    let n = 2f64.powf(iz);
    let x0 = (cx - w / 2.0 / scale).floor().max(0.0) as i64;
    let x1 = (cx + w / 2.0 / scale).floor().min(n - 1.0) as i64;
    let y0 = (cy - h / 2.0 / scale).floor().max(0.0) as i64;
    let y1 = (cy + h / 2.0 / scale).floor().min(n - 1.0) as i64;
    let mut labels: Vec<(u32, String, f32, f32)> = Vec::new();
    for pass in 0..2 {
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let Some(shapes) = tile(z, tx as u32, ty as u32) else {
                    continue;
                };
                let ox = (tx as f64 - cx) * scale + w / 2.0;
                let oy = (ty as f64 - cy) * scale + h / 2.0;
                let at = |(px_, py_): (f32, f32)| {
                    (
                        (ox + f64::from(px_) * scale) as f32,
                        (oy + f64::from(py_) * scale) as f32,
                    )
                };
                for s in shapes.iter() {
                    if s.class == Class::Place {
                        if pass == 1 {
                            if let (Some(t), Some(pt)) =
                                (&s.label, s.parts.first().and_then(|r| r.first()))
                            {
                                let (x, y) = at(*pt);
                                labels.push((s.rank, t.clone(), x, y));
                            }
                        }
                        continue;
                    }
                    // Fills first, then lines.
                    if (pass == 0) != s.class.filled() {
                        continue;
                    }
                    p = p.begin_path();
                    for part in &s.parts {
                        for (i, pt) in part.iter().enumerate() {
                            let (x, y) = at(*pt);
                            p = if i == 0 {
                                p.move_to(x, y)
                            } else {
                                p.line_to(x, y)
                            };
                        }
                    }
                    p = if s.class.filled() {
                        p.fill_style(color(s.class, dark)).fill()
                    } else {
                        p.stroke_style(color(s.class, dark))
                            .line_width(s.class.width(zoom as f32))
                            .stroke()
                    };
                }
            }
        }
    }
    // The most important names first; one of each, never overlapping much.
    labels.sort_by_key(|l| l.0);
    let mut placed: Vec<(f32, f32)> = Vec::new();
    p = p
        .font("600 12px system-ui")
        .fill_style(color(Class::Place, dark));
    for (_, text, x, y) in labels {
        if placed
            .iter()
            .any(|(px_, py_)| (px_ - x).abs() < 90.0 && (py_ - y).abs() < 22.0)
        {
            continue;
        }
        placed.push((x, y));
        p = p.fill_text(&text, x - 3.2 * text.chars().count() as f32, y);
    }
    p
}

/// A map centred on `center` (latitude, longitude) at `zoom`; `interactive`
/// lets the user pan it (the centre follows) and zoom with buttons.
pub fn map(center: Signal<(f64, f64)>, zoom: Signal<f64>, interactive: bool) -> View {
    let size = signal((0.0f64, 0.0f64));
    let dx = animated_value(0.0);
    let dy = animated_value(0.0);
    let drawing = canvas(move || {
        let _ = revision().get();
        // Dark tiles when the user chose dark (the system setting can't be
        // read from a canvas; a light map reads well in either).
        let dark = crate::store::try_store().is_some_and(|s| {
            s.settings
                .with(|x| x.theme == crate::store::ThemeChoice::Dark)
        });
        paint(center.get(), zoom.get(), size.get(), dark)
    })
    .absolute()
    .inset_0()
    .bind_translate_x(&dx)
    .bind_translate_y(&dy);
    let mut frame = view()
        .overflow_hidden()
        .on_layout(move |b: &Bounds<Points>| {
            size.set((f64::from(b.size.width.0), f64::from(b.size.height.0)));
        })
        .child(drawing)
        .child(
            text("© OpenStreetMap contributors")
                .absolute()
                .right(px(4.0))
                .bottom(px(2.0))
                .text_size(px(9.0))
                .text_color(move || c::muted_fg()),
        );
    if interactive {
        frame = frame
            .pan_values(Some(&dx), Some(&dy))
            .on_pan_end(move |e: &PanEvent| {
                let z = zoom.get_untracked();
                let scale = TILE * 2f64.powf(z - z.floor());
                let (lat, lon) = center.get_untracked();
                let (x, y) = maps::project(lat, lon, z.floor());
                let x = x - f64::from(e.translation.x.0) / scale;
                let y = y - f64::from(e.translation.y.0) / scale;
                center.set(maps::unproject(x, y, z.floor()));
                dx.set(0.0);
                dy.set(0.0);
            })
            .child(
                view()
                    .absolute()
                    .right(px(10.0))
                    .top(px(10.0))
                    .gap_2()
                    .child(icon_btn(Icon::ZoomIn, "Zoom in", move || {
                        zoom.update(|z| *z = (*z + 1.0).min(18.0));
                    }))
                    .child(icon_btn(Icon::ZoomOut, "Zoom out", move || {
                        zoom.update(|z| *z = (*z - 1.0).max(2.0));
                    })),
            );
    }
    frame
}

/// Centres the picker on where the device is (D-0174): asked once, now; the system asks the
/// person's permission the first time.
fn locate(center: Signal<(f64, f64)>, zoom: Signal<f64>) {
    spawn(async move {
        match here().await {
            Ok((latitude, longitude)) => {
                center.try_set((latitude, longitude));
                zoom.try_set(16.0);
            }
            Err(Here::Denied) => {
                toast(
                    "Songbird isn't allowed to use your location. Allow it in your device's settings.",
                );
            }
            Err(Here::Unavailable) => {
                toast("Your location isn't available right now. Move the map instead.");
            }
        }
    });
}

/// Why there's no position.
enum Here {
    /// The person (or device management) said no.
    Denied,
    /// Location services are off, or no fix came.
    Unavailable,
}

/// One position, about a hundred metres precise (a place on a map, not a track): CoreLocation
/// through NSH 0.7's `nsh-extra` on macOS and iOS (D-0188), the device crate elsewhere. A fix
/// that hasn't come in 30 s is none, once the person has answered the system's prompt.
#[cfg(any(target_os = "macos", target_os = "ios"))]
async fn here() -> Result<(f64, f64), Here> {
    use nsh_extra::location::{self, Authorization, LocationError, LocationRequest};
    // Asked first, without a deadline: the person may take their time with the prompt.
    match location::request_authorization().await {
        Ok(a) if a.allows_reading() => {}
        Ok(Authorization::Denied | Authorization::Restricted) => return Err(Here::Denied),
        _ => return Err(Here::Unavailable),
    }
    let mut request = LocationRequest::default();
    request.timeout = Some(std::time::Duration::from_secs(30));
    match location::current(request).await {
        Ok(p) => Ok((p.latitude, p.longitude)),
        Err(e) if e.code == LocationError::DENIED => Err(Here::Denied),
        Err(_) => Err(Here::Unavailable),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
async fn here() -> Result<(f64, f64), Here> {
    use songbird_device::location::{self, Failure};
    let (tx, rx) = tokio::sync::oneshot::channel();
    location::current(Box::new(move |r| {
        let _ = tx.send(r);
    }));
    match rx.await.unwrap_or(Err(Failure::Unavailable)) {
        Ok(p) => Ok((p.latitude, p.longitude)),
        Err(Failure::Denied) => Err(Here::Denied),
        Err(Failure::Unavailable) => Err(Here::Unavailable),
    }
}

/// The location picker: move the map under the pin, then send that place.
pub fn pick(open: Signal<bool>, on_pick: impl Fn(f64, f64) + 'static) -> nsh_base::SheetElement {
    // Where the last pick was, else the middle of Europe at country scale.
    let center = signal((48.0, 12.0));
    let zoom = signal(5.0);
    let on_pick = Rc::new(on_pick);
    app_sheet(
        open,
        "Send a location",
        view()
            .gap_3()
            .px_4()
            .pb_4()
            .child(
                view()
                    .h(px(380.0))
                    .rounded(move || radius::control())
                    .overflow_hidden()
                    .child(map(center, zoom, true).absolute().inset_0())
                    .child(
                        view()
                            .absolute()
                            .inset_0()
                            .items_center()
                            .justify_center()
                            .child(
                                icon(Icon::MapPin, 34.0)
                                    .text_color(move || c::primary())
                                    .mb(px(34.0)),
                            ),
                    ),
            )
            .child(btn(
                "Use my location",
                Variant::Secondary,
                BtnSize::Md,
                move || locate(center, zoom),
            ))
            .child(muted(
                "Move the map so the pin marks the place. Only the person you send it to sees it.",
            ))
            .child(btn(
                "Send this location",
                Variant::Primary,
                BtnSize::Lg,
                move || {
                    let (lat, lon) = center.get_untracked();
                    open.set(false);
                    on_pick(lat, lon);
                },
            )),
    )
}
