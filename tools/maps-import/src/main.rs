//! `songbird-maps-import`: `OpenStreetMap` vector tiles into `RustFS` (D-0142).
//!
//! ```text
//! planetiler --download --area=europe --output=europe.mbtiles   # OpenMapTiles schema, z0–14
//! SONGBIRD_S3_ACCESS_KEY=… SONGBIRD_S3_SECRET_KEY=… \
//!   songbird-maps-import --mbtiles europe.mbtiles --s3 127.0.0.1:47900 --bucket songbird-media
//! ```
//!
//! `MBTiles` rows are TMS (y from the south); objects are XYZ (`maps/v1/z/x/y.mvt`, y from the
//! north), stored as raw Mapbox Vector Tiles (gzip removed here, so clients need no inflater).
//! The data is © `OpenStreetMap` contributors under the `ODbL`: apps show that attribution on
//! every map.
#![forbid(unsafe_code)]

use std::io::Read as _;
use std::sync::Arc;

use bytes::Bytes;
use clap::Parser;
use songbird_types::Secret;

#[derive(Parser, Debug)]
struct Args {
    /// The `MBTiles` file.
    #[arg(long)]
    mbtiles: std::path::PathBuf,
    /// `RustFS` (S3) `host:port`.
    #[arg(long)]
    s3: String,
    /// The bucket the api serves media from.
    #[arg(long)]
    bucket: String,
    /// Skip tiles deeper than this.
    #[arg(long, default_value_t = 14)]
    max_zoom: u8,
    /// Uploads in flight.
    #[arg(long, default_value_t = 32)]
    parallel: usize,
}

fn ungzip(data: Vec<u8>) -> Result<Vec<u8>, String> {
    if data.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::with_capacity(data.len() * 3);
        flate2::read::GzDecoder::new(data.as_slice())
            .take(16 << 20)
            .read_to_end(&mut out)
            .map_err(|e| format!("a tile does not inflate: {e}"))?;
        Ok(out)
    } else {
        Ok(data)
    }
}

/// TMS row to XYZ y.
fn xyz_y(z: u8, row: u32) -> Option<u32> {
    (1u32 << z).checked_sub(1)?.checked_sub(row)
}

#[tokio::main]
#[allow(
    clippy::many_single_char_names,
    reason = "the tile grid's z, x, y and the row"
)]
async fn main() -> Result<(), String> {
    let args = Args::parse();
    let env = |n: &str| std::env::var(n).map_err(|_| format!("{n} is not set"));
    let s3 = songbird_s3::Client::new(songbird_s3::Config::new(
        args.s3.clone(),
        args.bucket.clone(),
        Secret::new(env("SONGBIRD_S3_ACCESS_KEY")?),
        Secret::new(env("SONGBIRD_S3_SECRET_KEY")?),
    ))
    .map_err(|e| format!("s3: {e}"))?;
    let s3 = Arc::new(s3);
    let db = rusqlite::Connection::open_with_flags(
        &args.mbtiles,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("mbtiles: {e}"))?;
    let mut st = db
        .prepare(
            "SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles WHERE zoom_level <= ?1",
        )
        .map_err(|e| format!("mbtiles: {e}"))?;
    let mut rows = st
        .query([i64::from(args.max_zoom)])
        .map_err(|e| format!("mbtiles: {e}"))?;
    let gate = Arc::new(tokio::sync::Semaphore::new(args.parallel.max(1)));
    let mut tasks = tokio::task::JoinSet::new();
    let mut n = 0u64;
    while let Some(row) = rows.next().map_err(|e| format!("mbtiles: {e}"))? {
        let z: u8 = row
            .get::<_, i64>(0)
            .ok()
            .and_then(|v| u8::try_from(v).ok())
            .ok_or("bad zoom")?;
        let x: u32 = row
            .get::<_, i64>(1)
            .ok()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or("bad column")?;
        let r: u32 = row
            .get::<_, i64>(2)
            .ok()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or("bad row")?;
        let data: Vec<u8> = row.get(3).map_err(|e| format!("mbtiles: {e}"))?;
        let y = xyz_y(z, r).ok_or("row outside the grid")?;
        let tile = ungzip(data)?;
        if tile.is_empty() {
            continue;
        }
        let permit = Arc::clone(&gate)
            .acquire_owned()
            .await
            .map_err(|e| e.to_string())?;
        let s3 = Arc::clone(&s3);
        tasks.spawn(async move {
            let key = format!("maps/v1/{z}/{x}/{y}.mvt");
            let r = s3.put_object_bytes(&key, Bytes::from(tile)).await;
            drop(permit);
            r.map_err(|e| format!("{key}: {e}"))
        });
        n += 1;
        while let Some(done) = tasks.try_join_next() {
            done.map_err(|e| e.to_string())??;
        }
    }
    while let Some(done) = tasks.join_next().await {
        done.map_err(|e| e.to_string())??;
    }
    #[allow(clippy::print_stdout, reason = "an operator tool's report")]
    {
        println!("{n} tiles loaded into {}/maps/v1", args.bucket);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tms_rows_flip_and_gzip_inflates() {
        assert_eq!(xyz_y(0, 0), Some(0));
        assert_eq!(xyz_y(2, 0), Some(3));
        assert_eq!(xyz_y(2, 4), None);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz, b"tile").unwrap_or_default();
        assert_eq!(
            ungzip(gz.finish().unwrap_or_default()).unwrap_or_default(),
            b"tile"
        );
        assert_eq!(ungzip(b"raw".to_vec()).unwrap_or_default(), b"raw");
    }
}
