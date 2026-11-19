//! Map tiles (D-0142): OpenStreetMap vector tiles (the OpenMapTiles schema, built with Planetiler
//! and loaded into RustFS by `tools/maps-import`), served from Songbird's own EU storage so no
//! map company learns where anyone looks.
//!
//! `GET /v1/maps/v1/<z>/<x>/<y>`, unsigned like attachment downloads (D-0079): a tile request
//! names no account and no device, only the area, and is budgeted per IP range (the media
//! serving budget, kind `t`). The answer is the raw Mapbox Vector Tile, 200; 204 for a tile with
//! nothing in it (open sea, beyond the imported region). Tiles are public data, cacheable.

use std::sync::Arc;

use http_body_util::{BodyExt as _, StreamBody};
use hyper::body::Incoming;
use hyper::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};

use crate::{App, Body, Refusal};

/// Deepest zoom served (the import's maximum; clients over-zoom beyond it).
pub const MAX_ZOOM: u8 = 14;
/// Where tiles live in the bucket.
pub const PREFIX: &str = "maps/v1";

/// The object name of a tile.
#[must_use]
pub fn object_name(z: u8, x: u32, y: u32) -> String {
    format!("{PREFIX}/{z}/{x}/{y}.mvt")
}

/// Parses `<z>/<x>/<y>`, refusing anything outside the tile grid.
#[must_use]
pub fn parse(rest: &str) -> Option<(u8, u32, u32)> {
    let mut it = rest.split('/');
    let z: u8 = it.next()?.parse().ok()?;
    let x: u32 = it.next()?.parse().ok()?;
    let y: u32 = it.next()?.parse().ok()?;
    if it.next().is_some() || z > MAX_ZOOM {
        return None;
    }
    let n = 1u32 << z;
    (x < n && y < n).then_some((z, x, y))
}

pub(crate) async fn route(
    app: &Arc<App>,
    req: Request<Incoming>,
) -> Result<Response<Body>, Refusal> {
    let status = |s| Refusal::Status(s);
    if req.method() != Method::GET {
        return Err(status(StatusCode::NOT_FOUND));
    }
    let Some((z, x, y)) = req
        .uri()
        .path()
        .strip_prefix("/v1/maps/v1/")
        .and_then(parse)
    else {
        return Err(status(StatusCode::NOT_FOUND));
    };
    let media = app
        .media
        .as_ref()
        .ok_or(status(StatusCode::SERVICE_UNAVAILABLE))?;
    if let Some(r) = crate::media::throttle(app, media, "t", crate::media::peer(&req)?).await {
        return Ok(r);
    }
    let got = match app.s3.get_object(&object_name(z, x, y), None).await {
        Ok(g) => g,
        Err(songbird_s3::Error::NotFound) => {
            let mut r = crate::empty(StatusCode::NO_CONTENT);
            r.headers_mut().insert(
                CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=604800"),
            );
            return Ok(r);
        }
        Err(_) => return Err(status(StatusCode::SERVICE_UNAVAILABLE)),
    };
    let mut r = Response::new(StreamBody::new(crate::media::Frames(got.body)).boxed_unsync());
    let h = r.headers_mut();
    h.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.mapbox-vector-tile"),
    );
    h.insert(CONTENT_LENGTH, HeaderValue::from(got.length));
    h.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=604800"),
    );
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_tile_grid_is_served() {
        assert_eq!(parse("0/0/0"), Some((0, 0, 0)));
        assert_eq!(parse("14/9000/5000"), Some((14, 9000, 5000)));
        for bad in [
            "1/2/0", "15/0/0", "3/1", "3/1/1/1", "a/b/c", "2/-1/0", "../../x",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        assert_eq!(object_name(3, 4, 5), "maps/v1/3/4/5.mvt");
    }
}
