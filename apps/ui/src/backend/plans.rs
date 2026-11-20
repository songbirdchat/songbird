//! Plans, Family seats, support and invites on the real backend (Prompt 11, M7; PROTOCOL
//! §14.3–§14.10).
//!
//! - **Entitlement**: `GET /v1/entitlements` gives the level, its last day and the account's
//!   standing; the Subscription page shows exactly that.
//! - **Buying on desktop** (Mollie, the EU provider): the app makes a random **subscriber
//!   secret** and keeps it; billing makes an anonymous checkout and the browser pays. The secret
//!   alone fetches the subscription's state, cancels it and fetches **anonymous tokens**
//!   (§14.4), finished against the token key this device **pinned** the first time and
//!   redeemed on the account. Billing never learns the account; `api` never learns the payment.
//! - **Family**: the owner keeps a management token and one seat link per member; each month's
//!   tokens beyond the owner's are sealed into the seats. A member keeps their seat link, fetches
//!   the sealed token without an account, and redeems it on their own.
//! - **Support**: tickets sealed to the support key, each with its own reply key kept here.
//! - **Invites**: the link carries the inviter's card in its fragment; the inviter's own codes'
//!   status is asked by hash.
//!
//! StoreKit 2 and Play Billing need the native hosts (deferred, like push): there the proof is
//! the store's signed transaction or purchase token instead of the subscriber secret.

use nsh_core::{platform::HttpRequest, storage::Storage};
use serde_json::Value;
use songbird_keys::provisioning::{Domain, ProvisioningKey, open_with, seal_with};
use songbird_tokens::family::Invite;
use songbird_tokens::token::{self, Info, IssueResponse, Kind, Redemption};

use super::{Problem, endpoint, ok_json, problem, put, send, signed, wire};

const K_SUBSCRIBER: &str = "sb.subscriber";
const K_TOKEN_PIN: &str = "sb.token_pin";
const K_FAMILY: &str = "sb.family";
const K_SEAT: &str = "sb.family_seat";
const K_TICKETS: &str = "sb.tickets";
const K_NEXT_TOKEN: &str = "sb.next_token";
const K_SUPPORT_PIN: &str = "sb.support_pin";
pub(super) const K_MY_INVITES: &str = "sb.my_invites";

/// The account as the server sees it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entitlement {
    /// `free` or `plus` (every Family seat is Plus).
    pub level: String,
    /// The last UTC day the level holds (`None` on Free).
    pub last_day: Option<u32>,
    /// `good`, `limited` or `suspended`.
    pub standing: String,
}

/// `GET /v1/entitlements`.
pub async fn entitlement() -> Result<Entitlement, Problem> {
    let (st, body) = signed("GET", "/v1/entitlements", None, false).await?;
    let v = ok_json(st, &body)?;
    Ok(parse_entitlement(&v))
}

fn parse_entitlement(v: &Value) -> Entitlement {
    Entitlement {
        level: v["level"].as_str().unwrap_or("free").to_owned(),
        last_day: v["last_day"].as_u64().and_then(|d| u32::try_from(d).ok()),
        standing: v["standing"].as_str().unwrap_or("good").to_owned(),
    }
}

/// The UTC day of a Unix time in milliseconds.
pub fn day_of(unix_ms: u64) -> u32 {
    u32::try_from(unix_ms / 86_400_000).unwrap_or(u32::MAX)
}

/// Today (UTC).
fn today() -> u32 {
    day_of(nsh_core::time::unix_ms() as u64)
}

// ---------------------------------------------------------------- billing (Mollie)

/// The subscriber secret (hex), made on first use and kept.
fn subscriber() -> String {
    Storage::get_item(K_SUBSCRIBER).unwrap_or_else(|| {
        let s = wire::hex(&wire::random::<32>());
        put(K_SUBSCRIBER, &s);
        s
    })
}

/// Whether this device has paid through the EU provider.
pub fn has_subscriber() -> bool {
    Storage::get_item(K_SUBSCRIBER).is_some()
}

async fn billing(path: &str, body: &Value) -> Result<(u16, Vec<u8>), Problem> {
    let b = serde_json::to_vec(body).map_err(|_| problem("Couldn't prepare the request."))?;
    send(
        HttpRequest::new("POST", format!("{}{path}", endpoint().billing))
            .header("content-type", "application/json")
            .header("content-length", b.len().to_string())
            .body(b),
    )
    .await
}

/// The plain-language meaning of a billing answer.
fn billing_problem(status: u16, body: &[u8]) -> Problem {
    let word = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_owned))
        .unwrap_or_default();
    problem(match (status, word.as_str()) {
        (402, _) => "The payment hasn't gone through yet. It can take a few minutes.",
        (409, "exists") => "This device already has a subscription. Manage it below.",
        (409, "renews") => "Your plan already renews.",
        (409, "expired") => "This plan has ended. Choose a plan to start again.",
        (409, "revoked") => "This payment was refunded or charged back, so it can't renew.",
        (409, "mandate") => {
            "The payment provider needs you to pay again. Choose a plan to start again."
        }
        (429, "cap") => "This subscription has had all its plans for this month.",
        (404, _) => "No subscription found for this device.",
        (503, _) => "The payment provider isn't answering. Try again in a moment.",
        _ => return super::explain(status, body),
    })
}

/// Starts a checkout: the URL to open in the browser.
pub async fn checkout(plan: &str, interval: &str) -> Result<String, Problem> {
    for _ in 0..2 {
        let (st, body) = billing(
            "/v1/billing/mollie/checkout",
            &serde_json::json!({ "plan": plan, "interval": interval, "subscriber": subscriber() }),
        )
        .await?;
        let word = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_owned));
        if st == 409 && word.as_deref() == Some("revoked") {
            // A refunded or charged-back subscription: start over under a new secret (billing
            // keeps the old row revoked; D-0134).
            Storage::remove_item(K_SUBSCRIBER);
            continue;
        }
        if st != 200 {
            return Err(billing_problem(st, &body));
        }
        return serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|v| v["checkout"].as_str().map(str::to_owned))
            .ok_or_else(|| problem("The payment provider gave no checkout page."));
    }
    Err(problem(
        "The payment provider refused the checkout. Try again later.",
    ))
}

/// A subscription's state, as billing has it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Subscription {
    /// `plus` or `family`.
    pub plan: String,
    /// `month` or `year`.
    pub interval: String,
    /// The last UTC day paid for.
    pub paid_through: u32,
    /// It renews (not cancelled).
    pub renews: bool,
    /// Refunded or charged back.
    pub revoked: bool,
}

/// The subscription of this device's secret, if any.
pub async fn subscription() -> Result<Option<Subscription>, Problem> {
    if !has_subscriber() {
        return Ok(None);
    }
    let (st, body) = billing(
        "/v1/billing/mollie/state",
        &serde_json::json!({ "subscriber": subscriber() }),
    )
    .await?;
    match st {
        200 => {
            let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            Ok(Some(Subscription {
                plan: v["plan"].as_str().unwrap_or_default().to_owned(),
                interval: v["interval"].as_str().unwrap_or_default().to_owned(),
                paid_through: v["paid_through"]
                    .as_u64()
                    .and_then(|d| u32::try_from(d).ok())
                    .unwrap_or(0),
                renews: v["renews"].as_bool().unwrap_or(false),
                revoked: v["revoked"].as_bool().unwrap_or(false),
            }))
        }
        404 => Ok(None),
        _ => Err(billing_problem(st, &body)),
    }
}

/// Cancels renewal (what was paid stays paid).
pub async fn cancel() -> Result<(), Problem> {
    let (st, body) = billing(
        "/v1/billing/mollie/cancel",
        &serde_json::json!({ "subscriber": subscriber() }),
    )
    .await?;
    if (200..300).contains(&st) {
        Ok(())
    } else {
        Err(billing_problem(st, &body))
    }
}

/// Turns renewal back on after a cancellation, while the paid period lasts (D-0169).
pub async fn resume() -> Result<(), Problem> {
    let (st, body) = billing(
        "/v1/billing/mollie/resume",
        &serde_json::json!({ "subscriber": subscriber() }),
    )
    .await?;
    if (200..300).contains(&st) {
        Ok(())
    } else {
        Err(billing_problem(st, &body))
    }
}

/// The token keys this build trusts: `(key ID, public key hex)`. A release build ships them
/// (filled at release from the deployment's `[api.tokens]`, D-0134) and refuses any other, so a
/// server cannot hand one user a key of their own and recognise their tokens. Empty in
/// development builds, which pin the first key they see instead (and refuse any other ID).
pub const SHIPPED_TOKEN_KEYS: &[(u8, &str)] = &[];

/// The support key this build trusts (X25519, hex); see [`SHIPPED_TOKEN_KEYS`].
pub const SHIPPED_SUPPORT_KEY: Option<&str> = None;

/// Whether `(id, pk)` is a token key this device may use: one shipped with the build, or in a
/// development build without shipped keys, the first one it ever saw (kept in app storage).
pub fn trusted_token_key(id: u8, pk: &[u8; 32]) -> Result<(), Problem> {
    let refused = || {
        problem(
            "The server offered a token key this app doesn't trust. Songbird refused it to keep your payment private.",
        )
    };
    if !SHIPPED_TOKEN_KEYS.is_empty() {
        return SHIPPED_TOKEN_KEYS
            .iter()
            .any(|(i, h)| *i == id && wire::unhex(h).as_deref() == Some(pk.as_slice()))
            .then_some(())
            .ok_or_else(refused);
    }
    if !cfg!(debug_assertions) {
        return Err(problem(
            "This build has no token keys, so plans can't be used.",
        ));
    }
    let pin =
        Storage::get_item(K_TOKEN_PIN).and_then(|s| serde_json::from_str::<(u8, String)>(&s).ok());
    match pin {
        Some((i, h)) if i == id && wire::unhex(&h).as_deref() == Some(pk.as_slice()) => Ok(()),
        Some(_) => Err(refused()),
        None => {
            put(
                K_TOKEN_PIN,
                &serde_json::to_string(&(id, wire::hex(pk))).unwrap_or_default(),
            );
            Ok(())
        }
    }
}

/// `GET /v1/tokens/keys`, checked by [`trusted_token_key`].
async fn pinned() -> Result<(u8, [u8; 32]), Problem> {
    let (st, body) = send(HttpRequest::get(format!(
        "{}/v1/tokens/keys",
        endpoint().api
    )))
    .await?;
    let v = ok_json(st, &body)?;
    let (id, pk) =
        parse_keys(&v).ok_or_else(|| problem("The server's token keys are malformed."))?;
    trusted_token_key(id, &pk)?;
    Ok((id, pk))
}

/// The support key: the shipped one, or in a development build the first one seen.
pub(super) async fn support_key() -> Result<[u8; 32], Problem> {
    if let Some(k) = SHIPPED_SUPPORT_KEY {
        return wire::unhex(k)
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| problem("This build's support key is damaged."));
    }
    if !cfg!(debug_assertions) {
        return Err(problem("This build has no support key."));
    }
    let (st, body) = send(HttpRequest::get(format!(
        "{}/v1/support/key",
        endpoint().api
    )))
    .await?;
    let k: [u8; 32] = ok_json(st, &body)?["key"]
        .as_str()
        .and_then(wire::unhex)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| problem("The server's support key is malformed."))?;
    match Storage::get_item(K_SUPPORT_PIN) {
        Some(h) if wire::unhex(&h).as_deref() == Some(k.as_slice()) => Ok(k),
        Some(_) => Err(problem(
            "The server's support key changed. Songbird refused it.",
        )),
        None => {
            put(K_SUPPORT_PIN, &wire::hex(&k));
            Ok(k)
        }
    }
}

fn parse_keys(v: &Value) -> Option<(u8, [u8; 32])> {
    let id = u8::try_from(v["current"].as_u64()?).ok()?;
    let pk = v["keys"]
        .as_array()?
        .iter()
        .find(|k| k["id"].as_u64() == Some(u64::from(id)))?["public"]
        .as_str()
        .and_then(wire::unhex)?
        .try_into()
        .ok()?;
    Some((id, pk))
}

/// `n` plan tokens for `month`, from billing on the subscriber secret.
async fn plan_tokens(month: u32, n: usize) -> Result<Vec<Redemption>, Problem> {
    let (id, pk) = pinned().await?;
    let info = Info {
        kind: Kind::Plan,
        level: 1,
        period: month,
    };
    let (pending, req) =
        token::request(id, info, n).map_err(|_| problem("Couldn't prepare the tokens."))?;
    let (st, body) = billing(
        "/v1/billing/tokens",
        &serde_json::json!({ "proof": { "mollie": subscriber() }, "request": req }),
    )
    .await?;
    if st != 200 {
        return Err(billing_problem(st, &body));
    }
    let resp: IssueResponse =
        serde_json::from_slice(&body).map_err(|_| problem("Billing's answer is malformed."))?;
    pending
        .finish(&pk, &resp)
        .map_err(|_| problem("Billing's tokens didn't verify. Songbird refused them."))
}

/// `POST /v1/entitlements/redeem`.
pub async fn redeem(t: &Redemption) -> Result<(), Problem> {
    let body = serde_json::to_vec(t).map_err(|_| problem("Couldn't prepare the token."))?;
    let (st, b) = signed("POST", "/v1/entitlements/redeem", Some(body), true).await?;
    let spent = serde_json::from_slice::<Value>(&b)
        .ok()
        .is_some_and(|v| v["error"] == "spent");
    match st {
        200 => Ok(()),
        // Already redeemed (a retry after a lost answer): the plan is there. Any other 409
        // (`period`, `invalid`) is a refusal.
        409 if spent => Ok(()),
        _ => Err(super::explain(st, &b)),
    }
}

/// A random moment in the first two days of `month` (Unix ms): when a kept token is redeemed,
/// never next to its issuance (D-0134).
fn redeem_moment(month: u32) -> u64 {
    u64::from(token::first_day(month)) * 86_400_000
        + u64::from(u32::from_le_bytes(wire::random::<4>()) % 172_800_000)
}

/// The run after a payment: this month's tokens (one for this account, one per Family seat),
/// redeemed and sealed into the seats; next month's own token kept to redeem at a random
/// moment of its first two days. (A first purchase is redeemed at once: the user is waiting
/// for it. Billing and the registry keep only days, so stored records cannot be joined.)
pub async fn renew() -> Result<(), Problem> {
    let month = token::month_of(today());
    let seats = family().map(|f| f.seats.len()).unwrap_or(0);
    let mut tokens = plan_tokens(month, 1 + seats).await?;
    let own = tokens.remove(0);
    redeem(&own).await?;
    if let Some(f) = family() {
        for (seat, t) in f.seats.iter().zip(tokens) {
            seat_put(&f, seat, &t).await?;
        }
    }
    // Next month's own token, if billing will give it yet (paid on its first day).
    if let Ok(mut next) = plan_tokens(month + 1, 1).await
        && let Some(t) = next.pop()
    {
        let at = redeem_moment(month + 1);
        put(
            K_NEXT_TOKEN,
            &serde_json::to_string(&(t, at)).unwrap_or_default(),
        );
    }
    Ok(())
}

/// Redeems a kept token once its moment has come (called at start-up and when the plans page
/// opens).
pub async fn redeem_kept() -> Result<(), Problem> {
    let Some((t, at)) = Storage::get_item(K_NEXT_TOKEN)
        .and_then(|s| serde_json::from_str::<(Redemption, u64)>(&s).ok())
    else {
        return Ok(());
    };
    let month = token::month_of(today());
    if t.period > month || (nsh_core::time::unix_ms() as u64) < at {
        return Ok(());
    }
    if t.period == month {
        redeem(&t).await?;
    }
    Storage::remove_item(K_NEXT_TOKEN);
    Ok(())
}

// ---------------------------------------------------------------- family

/// The owner's family: the management token and the seats.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Family {
    /// The family ID (hex).
    pub id: String,
    /// The management token (hex): never leaves this device but in its own header.
    manage: String,
    /// The seats.
    pub seats: Vec<Seat>,
}

/// One member's seat, as the owner keeps it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Seat {
    /// The name the owner gave it (on this device only).
    pub name: String,
    /// The link to share (it holds the seat's secret).
    pub link: String,
}

/// The owner's family, if this device made one.
pub fn family() -> Option<Family> {
    serde_json::from_str(&Storage::get_item(K_FAMILY)?).ok()
}

fn save_family(f: &Family) {
    put(K_FAMILY, &serde_json::to_string(f).unwrap_or_default());
}

async fn mailbox(
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: Option<Value>,
) -> Result<(u16, Vec<u8>), Problem> {
    let mut r = HttpRequest::new(method, format!("{}{path}", endpoint().api));
    for (k, v) in headers {
        r = r.header(*k, v.clone());
    }
    if let Some(b) = body {
        let b = serde_json::to_vec(&b).map_err(|_| problem("Couldn't prepare the request."))?;
        r = r
            .header("content-type", "application/json")
            .header("content-length", b.len().to_string())
            .body(b);
    }
    send(r).await
}

/// Makes the family mailbox (once).
pub async fn family_create() -> Result<Family, Problem> {
    if let Some(f) = family() {
        return Ok(f);
    }
    let manage = wire::random::<32>();
    let (st, body) = mailbox(
        "POST",
        "/v1/family",
        &[],
        Some(serde_json::json!({
            "manage": wire::hex(&songbird_tokens::family::manage_hash(&manage)),
        })),
    )
    .await?;
    let v = ok_json(st, &body)?;
    let f = Family {
        id: v["family"]
            .as_str()
            .ok_or_else(|| problem("The server made no family."))?
            .to_owned(),
        manage: wire::hex(&manage),
        seats: Vec::new(),
    };
    save_family(&f);
    Ok(f)
}

fn family_id(f: &Family) -> Result<[u8; 16], Problem> {
    wire::unhex(&f.id)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| problem("This device's family is damaged."))
}

/// Seals `t` into `seat` (the server sees the seat's bearer hash and ciphertext only).
async fn seat_put(f: &Family, seat: &Seat, t: &Redemption) -> Result<(), Problem> {
    let inv = Invite::parse(&seat.link).ok_or_else(|| problem("A seat's link is damaged."))?;
    let sealed = inv
        .seal(&serde_json::to_vec(t).unwrap_or_default())
        .map_err(|_| problem("Couldn't seal the seat."))?;
    let (st, body) = mailbox(
        "PUT",
        &format!("/v1/family/{}/slots/{}", f.id, wire::hex(&inv.slot)),
        &[("x-songbird-family-token", f.manage.clone())],
        Some(serde_json::json!({
            "auth": wire::hex(&songbird_tokens::family::bearer_hash(&inv.bearer())),
            "sealed": wire::hex(&sealed),
        })),
    )
    .await?;
    if (200..300).contains(&st) {
        Ok(())
    } else if st == 409 {
        Err(problem("A family has five seats at most."))
    } else {
        Err(super::explain(st, &body))
    }
}

/// A new seat: its link to share. Filled now if this month's tokens can be had, else at the
/// next renewal.
pub async fn family_invite(name: &str) -> Result<Seat, Problem> {
    let mut f = family_create().await?;
    if f.seats.len() >= 5 {
        return Err(problem("A family has five seats at most."));
    }
    let inv = Invite::new(family_id(&f)?).map_err(|_| problem("Couldn't make a seat."))?;
    let seat = Seat {
        name: name.to_owned(),
        link: inv.link(),
    };
    let month = token::month_of(today());
    if let Ok(mut t) = plan_tokens(month, 1).await
        && let Some(t) = t.pop()
    {
        seat_put(&f, &seat, &t).await?;
    }
    f.seats.push(seat.clone());
    save_family(&f);
    Ok(seat)
}

/// Removes a seat at once (the month the member already redeemed stays theirs).
pub async fn family_remove(link: &str) -> Result<(), Problem> {
    let mut f = family().ok_or_else(|| problem("This device has no family."))?;
    let inv = Invite::parse(link).ok_or_else(|| problem("A seat's link is damaged."))?;
    let (st, body) = mailbox(
        "DELETE",
        &format!("/v1/family/{}/slots/{}", f.id, wire::hex(&inv.slot)),
        &[("x-songbird-family-token", f.manage.clone())],
        None,
    )
    .await?;
    if !(200..300).contains(&st) && st != 404 {
        return Err(super::explain(st, &body));
    }
    f.seats.retain(|s| s.link != link);
    save_family(&f);
    Ok(())
}

/// A member: keeps the seat link they were sent.
pub fn join_family(link: &str) -> Result<(), Problem> {
    Invite::parse(link.trim()).ok_or_else(|| problem("That isn't a Songbird family link."))?;
    put(K_SEAT, link.trim());
    Ok(())
}

/// Whether this device holds a seat in someone's family.
pub fn has_seat() -> bool {
    Storage::get_item(K_SEAT).is_some()
}

/// A member: fetches the seat's sealed token and redeems it on this account. `false` when the
/// seat holds nothing yet.
pub async fn claim_seat() -> Result<bool, Problem> {
    let link = Storage::get_item(K_SEAT).ok_or_else(|| problem("This device has no seat."))?;
    let inv = Invite::parse(&link).ok_or_else(|| problem("The seat link is damaged."))?;
    let (st, body) = mailbox(
        "GET",
        &format!(
            "/v1/family/{}/slots/{}",
            wire::hex(&inv.family),
            wire::hex(&inv.slot)
        ),
        &[("x-songbird-family-bearer", wire::hex(&inv.bearer()))],
        None,
    )
    .await?;
    if st == 404 {
        return Err(problem(
            "This seat is no longer in the family. Ask the family's owner for a new link.",
        ));
    }
    let v = ok_json(st, &body)?;
    let Some(sealed) = v["sealed"].as_str().and_then(wire::unhex) else {
        return Ok(false);
    };
    let t: Redemption = inv
        .open(&sealed)
        .and_then(|p| serde_json::from_slice(&p).ok())
        .ok_or_else(|| problem("The seat's token doesn't open with this link."))?;
    redeem(&t).await?;
    Ok(true)
}

/// A member leaves: the seat is gone at once.
pub async fn leave_family() -> Result<(), Problem> {
    let Some(link) = Storage::get_item(K_SEAT) else {
        return Ok(());
    };
    if let Some(inv) = Invite::parse(&link) {
        let (st, body) = mailbox(
            "DELETE",
            &format!(
                "/v1/family/{}/slots/{}",
                wire::hex(&inv.family),
                wire::hex(&inv.slot)
            ),
            &[("x-songbird-family-bearer", wire::hex(&inv.bearer()))],
            None,
        )
        .await?;
        if !(200..300).contains(&st) && st != 404 {
            return Err(super::explain(st, &body));
        }
    }
    Storage::remove_item(K_SEAT);
    Ok(())
}

// ---------------------------------------------------------------- support

/// What the user sends: their text and, only with consent after the preview, the scrubbed logs.
pub fn ticket_message(text: &str, logs: Option<&str>) -> Vec<u8> {
    let mut v = serde_json::json!({
        "text": text,
        "app": format!("Songbird {}", env!("CARGO_PKG_VERSION")),
    });
    if let Some(l) = logs {
        v["logs"] = Value::String(l.to_owned());
    }
    serde_json::to_vec(&v).unwrap_or_default()
}

/// A ticket this device opened: what reads and answers it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Ticket {
    /// Its ID (hex).
    pub id: String,
    /// The bearer (hex).
    bearer: String,
    /// The reply key's private half (hex).
    reply: String,
    /// `help`, `appeal` or `billing`.
    pub kind: String,
}

/// The tickets this device has open.
pub fn tickets() -> Vec<Ticket> {
    Storage::get_item(K_TICKETS)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_tickets(t: &[Ticket]) {
    put(K_TICKETS, &serde_json::to_string(t).unwrap_or_default());
}

/// Opens a ticket. `about_account` signs it (attaches the account: needed for appeals, gives
/// priority on Plus); otherwise nobody is attached.
pub async fn open_ticket(
    kind: &str,
    text: &str,
    logs: Option<&str>,
    about_account: bool,
) -> Result<Ticket, Problem> {
    let support = support_key().await?;
    let reply_private = wire::random::<32>();
    let reply = ProvisioningKey::from_private(&reply_private)
        .map_err(|_| problem("Couldn't make a reply key."))?;
    let sealed = seal_with(Domain::Support, &support, &ticket_message(text, logs))
        .map_err(|_| problem("The server's support key isn't usable."))?;
    let req = serde_json::json!({
        "kind": kind,
        "reply_key": wire::hex(&reply.public()),
        "sealed": wire::hex(&sealed),
    });
    let b = serde_json::to_vec(&req).unwrap_or_default();
    let (st, body) = if about_account {
        signed("POST", "/v1/support/tickets", Some(b), true).await?
    } else {
        send(
            HttpRequest::new("POST", format!("{}/v1/support/tickets", endpoint().api))
                .header("content-type", "application/json")
                .header("content-length", b.len().to_string())
                .body(b),
        )
        .await?
    };
    let v = ok_json(st, &body)?;
    let t = Ticket {
        id: v["ticket"].as_str().unwrap_or_default().to_owned(),
        bearer: v["bearer"].as_str().unwrap_or_default().to_owned(),
        reply: wire::hex(&reply_private),
        kind: kind.to_owned(),
    };
    let mut all = tickets();
    all.push(t.clone());
    save_tickets(&all);
    Ok(t)
}

/// One message of a ticket, opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketMessage {
    /// From the user (this device) or from support.
    pub from_support: bool,
    /// Its text (the user's own messages show as "sent": they are sealed to support).
    pub text: Option<String>,
}

/// Reads a ticket: support's replies opened with its reply key.
pub async fn read_ticket(t: &Ticket) -> Result<Vec<TicketMessage>, Problem> {
    let (st, body) = send(
        HttpRequest::get(format!("{}/v1/support/tickets/{}", endpoint().api, t.id))
            .header("x-songbird-ticket-bearer", t.bearer.clone()),
    )
    .await?;
    if st == 404 {
        save_tickets(
            &tickets()
                .into_iter()
                .filter(|x| x.id != t.id)
                .collect::<Vec<_>>(),
        );
        return Err(problem("This request was closed."));
    }
    let v = ok_json(st, &body)?;
    let key = wire::unhex(&t.reply)
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
        .and_then(|k| ProvisioningKey::from_private(&k).ok())
        .ok_or_else(|| problem("This request's key is damaged."))?;
    Ok(v["messages"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let from_support = m["from"] == "staff";
            let text = from_support
                .then(|| m["sealed"].as_str().and_then(wire::unhex))
                .flatten()
                .and_then(|s| open_with(Domain::SupportReply, &key, &s).ok())
                .and_then(|p| serde_json::from_slice::<Value>(&p).ok())
                .and_then(|v| v["text"].as_str().map(str::to_owned));
            TicketMessage { from_support, text }
        })
        .collect())
}

/// Closes a ticket: the server deletes it and every message, logs included.
pub async fn close_ticket(t: &Ticket) -> Result<(), Problem> {
    let (st, body) = send(
        HttpRequest::new(
            "DELETE",
            format!("{}/v1/support/tickets/{}", endpoint().api, t.id),
        )
        .header("x-songbird-ticket-bearer", t.bearer.clone()),
    )
    .await?;
    if !(200..300).contains(&st) && st != 404 {
        return Err(super::explain(st, &body));
    }
    save_tickets(
        &tickets()
            .into_iter()
            .filter(|x| x.id != t.id)
            .collect::<Vec<_>>(),
    );
    Ok(())
}

// ---------------------------------------------------------------- invites

/// The personal invite link: this account's card, all in the fragment. `profile_key` and
/// `fingerprint` come from the profile and identity key.
pub fn invite_link(
    account: [u8; 16],
    profile_key: [u8; 32],
    fingerprint: [u8; 32],
    code: Option<[u8; 15]>,
) -> String {
    songbird_proto::invite_link::InviteLink {
        code,
        inviter: songbird_types::AccountId::from_bytes(account),
        profile_key,
        fingerprint,
    }
    .to_link()
}

/// This account's own invite link (real backend): its ID, its profile key and the fingerprint
/// of its identity key, all in the fragment. `None` before registration.
pub fn my_invite_link() -> Option<String> {
    let account: [u8; 16] = wire::unhex(&super::live().with_untracked(|l| l.account.clone())?)?
        .try_into()
        .ok()?;
    let seed: [u8; 32] = wire::unhex(&Storage::get_item(super::K_IDENTITY)?)?
        .try_into()
        .ok()?;
    let identity = wire::public(&wire::ed25519(&seed)?);
    Some(invite_link(
        account,
        super::profile::profile_key(),
        wire::sha256(&identity),
        None,
    ))
}

/// The invite codes this device made (display form).
pub fn my_invites() -> Vec<String> {
    Storage::get_item(K_MY_INVITES)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// This account's codes and whether each is used: codes are made up to the allowance (at most
/// five at a time), then their status asked by hash.
pub async fn invite_codes() -> Result<Vec<(String, bool)>, Problem> {
    let (mut left, _) = super::invite_allowance().await?;
    let mut fresh = my_invites().len();
    while left > 0 && fresh < 5 {
        super::create_invite().await?;
        left -= 1;
        fresh += 1;
    }
    let codes = my_invites();
    let raw: Vec<[u8; 15]> = codes
        .iter()
        .filter_map(|c| songbird_proto::registration::invite::parse(c))
        .collect();
    let live = invites_live(&raw).await?;
    Ok(codes.into_iter().zip(live).map(|(c, l)| (c, !l)).collect())
}

/// `POST /v1/invites/status`: whether each of these codes still has a use left.
pub async fn invites_live(codes: &[[u8; 15]]) -> Result<Vec<bool>, Problem> {
    if codes.is_empty() {
        return Ok(Vec::new());
    }
    let hashes: Vec<String> = codes.iter().map(|c| wire::hex(&invite_hash(c))).collect();
    let mut out = Vec::with_capacity(codes.len());
    for chunk in hashes.chunks(50) {
        let b = serde_json::to_vec(&serde_json::json!({ "codes": chunk })).unwrap_or_default();
        let (st, body) = signed("POST", "/v1/invites/status", Some(b), true).await?;
        let v = ok_json(st, &body)?;
        out.extend(
            v["live"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .map(|x| x.as_bool().unwrap_or(false)),
        );
    }
    Ok(out)
}

/// `SHA-256("songbird.v1/invite\0" || code)`, as the registry stores it.
pub fn invite_hash(code: &[u8; 15]) -> [u8; 32] {
    let mut b = b"songbird.v1/invite\0".to_vec();
    b.extend_from_slice(code);
    wire::sha256(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entitlement_and_keys_parse() {
        let e = parse_entitlement(&serde_json::json!({
            "level": "plus", "last_day": 20_800, "standing": "limited",
        }));
        assert_eq!(e.level, "plus");
        assert_eq!(e.last_day, Some(20_800));
        assert_eq!(e.standing, "limited");
        assert_eq!(parse_entitlement(&serde_json::json!({})).standing, "good");
        let k = serde_json::json!({
            "current": 2,
            "keys": [{ "id": 1, "public": "00".repeat(32) }, { "id": 2, "public": "11".repeat(32) }],
        });
        assert_eq!(parse_keys(&k), Some((2, [0x11; 32])));
        assert_eq!(
            parse_keys(&serde_json::json!({ "current": 3, "keys": [] })),
            None
        );
        assert_eq!(day_of(86_400_000 * 3 + 5), 3);
        // A kept token is redeemed within its month's first two days.
        let m = token::month_of(20_776);
        let at = redeem_moment(m);
        assert_eq!(token::month_of(day_of(at)), m);
        assert!(day_of(at) < token::first_day(m) + 2);
    }

    #[test]
    fn a_ticket_message_carries_logs_only_when_given() {
        let without: Value = serde_json::from_slice(&ticket_message("hi", None)).unwrap();
        assert_eq!(without["text"], "hi");
        assert!(without.get("logs").is_none());
        let with: Value = serde_json::from_slice(&ticket_message("hi", Some("scrubbed"))).unwrap();
        assert_eq!(with["logs"], "scrubbed");
    }

    #[test]
    fn invite_links_and_hashes_match_the_server() {
        let l = invite_link([1; 16], [2; 32], [3; 32], Some([4; 15]));
        let p = songbird_proto::invite_link::InviteLink::parse(&l).unwrap();
        assert_eq!(p.code, Some([4; 15]));
        assert_eq!(p.inviter.as_bytes(), &[1; 16]);
        // The registry's hash (`songbird_accounts::invite_hash`), recomputed here.
        let mut b = b"songbird.v1/invite\0".to_vec();
        b.extend_from_slice(&[4; 15]);
        assert_eq!(invite_hash(&[4; 15]), wire::sha256(&b));
    }
}
