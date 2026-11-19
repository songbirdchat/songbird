//! Maps on the device (D-0142): Web Mercator tile math, a Mapbox Vector Tile decoder, and
//! Songbird's basemap style for the `OpenMapTiles` schema, giving shapes in tile coordinates
//! (0 to 1) that any canvas can draw. Tiles come from Songbird's own endpoint
//! (`GET /v1/maps/v1/<z>/<x>/<y>`); the data is © `OpenStreetMap` contributors (`ODbL`), shown on
//! every map.
//!
//! The decoder is bounded: at most [`MAX_TILE`] bytes, [`MAX_FEATURES`] features and
//! [`MAX_POINTS`] points per tile, so a hostile tile costs a fixed amount of work.

use prost::Message as _;

/// Largest tile accepted.
pub const MAX_TILE: usize = 4 << 20;
/// Most features drawn from one tile.
pub const MAX_FEATURES: usize = 20_000;
/// Most points decoded from one tile.
pub const MAX_POINTS: usize = 400_000;
/// Tiles exist to this zoom; deeper views scale them.
pub const MAX_ZOOM: u8 = 14;

/// The vector tile schema (Mapbox Vector Tile 2.1), only what is drawn.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Tile {
    /// Layers.
    #[prost(message, repeated, tag = "3")]
    pub layers: Vec<Layer>,
}

/// A layer.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Layer {
    /// Its name (`water`, `transportation`, …).
    #[prost(string, tag = "1")]
    pub name: String,
    /// Features.
    #[prost(message, repeated, tag = "2")]
    pub features: Vec<Feature>,
    /// Tag keys.
    #[prost(string, repeated, tag = "3")]
    pub keys: Vec<String>,
    /// Tag values.
    #[prost(message, repeated, tag = "4")]
    pub values: Vec<Value>,
    /// Coordinate extent (4096 by default).
    #[prost(uint32, optional, tag = "5")]
    pub extent: Option<u32>,
}

/// A feature.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Feature {
    /// Key/value index pairs.
    #[prost(uint32, repeated, tag = "2")]
    pub tags: Vec<u32>,
    /// 1 point, 2 line, 3 polygon.
    #[prost(int32, optional, tag = "3")]
    pub r#type: Option<i32>,
    /// Command-encoded geometry.
    #[prost(uint32, repeated, tag = "4")]
    pub geometry: Vec<u32>,
}

/// A tag value.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Value {
    /// A string.
    #[prost(string, optional, tag = "1")]
    pub string_value: Option<String>,
    /// An integer.
    #[prost(int64, optional, tag = "4")]
    pub int_value: Option<i64>,
    /// An unsigned integer.
    #[prost(uint64, optional, tag = "5")]
    pub uint_value: Option<u64>,
}

/// What a shape is, which decides how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    /// Land use and cover drawn green (parks, forest, grass).
    Green,
    /// Built-up areas.
    Urban,
    /// Water bodies.
    Water,
    /// Buildings.
    Building,
    /// Rivers and streams.
    Waterway,
    /// Country and region borders.
    Boundary,
    /// Paths and tracks.
    Path,
    /// Minor streets.
    Minor,
    /// Secondary and tertiary roads.
    Secondary,
    /// Primary and trunk roads.
    Primary,
    /// Motorways.
    Motorway,
    /// Railways.
    Rail,
    /// Place names (cities, towns, suburbs).
    Place,
}

impl Class {
    /// Stroke width in points at zoom `z` (0 for fills and labels).
    #[must_use]
    pub fn width(self, z: f32) -> f32 {
        let grow = |base: f32| base * (1.0 + ((z - 10.0).max(0.0) * 0.45));
        match self {
            Self::Motorway => grow(1.6),
            Self::Primary => grow(1.3),
            Self::Secondary => grow(1.0),
            Self::Minor | Self::Rail => grow(0.6),
            Self::Path => 0.6,
            Self::Waterway => grow(0.8),
            Self::Boundary => 0.8,
            _ => 0.0,
        }
    }

    /// Whether it is filled (else stroked, or a label).
    #[must_use]
    pub fn filled(self) -> bool {
        matches!(
            self,
            Self::Green | Self::Urban | Self::Water | Self::Building
        )
    }
}

/// One drawable shape in tile coordinates (0 to 1 across the tile).
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// What it is.
    pub class: Class,
    /// Rings (fills) or lines (strokes); one point for a label.
    pub parts: Vec<Vec<(f32, f32)>>,
    /// A label's text.
    pub label: Option<String>,
    /// A label's importance (lower first).
    pub rank: u32,
}

/// Decodes and styles a tile. `None` for a tile that does not parse or breaks the bounds.
#[must_use]
pub fn shapes(bytes: &[u8], z: u8) -> Option<Vec<Shape>> {
    if bytes.len() > MAX_TILE {
        return None;
    }
    let tile = Tile::decode(bytes).ok()?;
    let mut out = Vec::new();
    let mut points = 0usize;
    let mut features = 0usize;
    for layer in &tile.layers {
        let extent = f32::from(u16::try_from(layer.extent.unwrap_or(4096)).ok()?.max(1));
        for f in &layer.features {
            features += 1;
            if features > MAX_FEATURES {
                return None;
            }
            let tag = |k: &str| -> Option<&Value> {
                f.tags.as_chunks::<2>().0.iter().find_map(|[ki, vi]| {
                    let key = layer.keys.get(usize::try_from(*ki).ok()?)?;
                    (key == k)
                        .then(|| layer.values.get(usize::try_from(*vi).ok()?))
                        .flatten()
                })
            };
            let text = |k: &str| tag(k).and_then(|v| v.string_value.clone());
            let class = match layer.name.as_str() {
                "water" => Some(Class::Water),
                "waterway" => Some(Class::Waterway),
                "park" => Some(Class::Green),
                "landcover" => match text("class").as_deref() {
                    Some("wood" | "grass" | "farmland" | "wetland") => Some(Class::Green),
                    _ => None,
                },
                "landuse" => match text("class").as_deref() {
                    Some("residential" | "commercial" | "industrial" | "retail") => {
                        Some(Class::Urban)
                    }
                    Some("cemetery" | "stadium" | "pitch" | "playground") => Some(Class::Green),
                    _ => None,
                },
                "building" if z >= 14 => Some(Class::Building),
                "boundary" => match tag("admin_level").and_then(|v| {
                    v.int_value
                        .or(v.uint_value.and_then(|u| i64::try_from(u).ok()))
                }) {
                    Some(l) if l <= 4 => Some(Class::Boundary),
                    _ => None,
                },
                "transportation" => match text("class").as_deref() {
                    Some("motorway") => Some(Class::Motorway),
                    Some("trunk" | "primary") => Some(Class::Primary),
                    Some("secondary" | "tertiary") => Some(Class::Secondary),
                    Some("minor" | "service") if z >= 13 => Some(Class::Minor),
                    Some("path" | "track") if z >= 14 => Some(Class::Path),
                    Some("rail") => Some(Class::Rail),
                    _ => None,
                },
                "place" => match text("class").as_deref() {
                    Some("country" | "state" | "city" | "town") => Some(Class::Place),
                    Some("village" | "suburb" | "neighbourhood") if z >= 12 => Some(Class::Place),
                    _ => None,
                },
                _ => None,
            };
            let Some(class) = class else { continue };
            let parts = geometry(&f.geometry, extent, &mut points)?;
            if parts.is_empty() {
                continue;
            }
            let (label, rank) = if class == Class::Place {
                let name = text("name:latin").or_else(|| text("name"));
                let rank = tag("rank")
                    .and_then(|v| {
                        v.int_value
                            .or(v.uint_value.and_then(|u| i64::try_from(u).ok()))
                    })
                    .unwrap_or(99);
                (name, u32::try_from(rank).unwrap_or(99))
            } else {
                (None, 0)
            };
            if class == Class::Place && label.is_none() {
                continue;
            }
            out.push(Shape {
                class,
                parts,
                label,
                rank,
            });
        }
    }
    // Fills under strokes under labels, each class in one pass.
    out.sort_by_key(|s| s.class);
    Some(out)
}

fn zigzag(v: u32) -> i32 {
    // (v >> 1) ^ -(v & 1), without casts.
    let half = i32::try_from(v >> 1).unwrap_or(i32::MAX);
    if v & 1 == 0 { half } else { -half - 1 }
}

fn geometry(cmds: &[u32], extent: f32, points: &mut usize) -> Option<Vec<Vec<(f32, f32)>>> {
    let mut parts: Vec<Vec<(f32, f32)>> = Vec::new();
    let (mut x, mut y) = (0i32, 0i32);
    let mut i = 0usize;
    while let Some(&c) = cmds.get(i) {
        i += 1;
        let id = c & 7;
        let count = usize::try_from(c >> 3).ok()?;
        match id {
            1 | 2 => {
                for _ in 0..count {
                    let dx = zigzag(*cmds.get(i)?);
                    let dy = zigzag(*cmds.get(i + 1)?);
                    i += 2;
                    x = x.saturating_add(dx);
                    y = y.saturating_add(dy);
                    *points += 1;
                    if *points > MAX_POINTS {
                        return None;
                    }
                    #[allow(
                        clippy::cast_precision_loss,
                        clippy::as_conversions,
                        reason = "tile coordinates are small integers"
                    )]
                    let p = (x as f32 / extent, y as f32 / extent);
                    if id == 1 {
                        parts.push(vec![p]);
                    } else {
                        parts.last_mut()?.push(p);
                    }
                }
            }
            7 => {
                if let Some(first) = parts.last().and_then(|p| p.first().copied()) {
                    parts.last_mut()?.push(first);
                }
            }
            _ => return None,
        }
    }
    Some(parts)
}

/// A point on the world map at zoom `z`, in tile units (the tile is the integer part).
#[must_use]
pub fn project(lat: f64, lon: f64, z: f64) -> (f64, f64) {
    let n = 2f64.powf(z);
    let lat = lat.clamp(-85.051_128, 85.051_128).to_radians();
    let x = (lon + 180.0) / 360.0 * n;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * n;
    (x, y)
}

/// The inverse of [`project`].
#[must_use]
pub fn unproject(x: f64, y: f64, z: f64) -> (f64, f64) {
    let n = 2f64.powf(z);
    let lon = x / n * 360.0 - 180.0;
    let lat = (std::f64::consts::PI * (1.0 - 2.0 * y / n))
        .sinh()
        .atan()
        .to_degrees();
    (lat, lon)
}

/// `LocationPin`'s fixed-point degrees (× 10⁷), clamped to the valid range.
#[must_use]
pub fn e7(deg: f64, max: f64) -> i32 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "clamped to ±1.8e9"
    )]
    let v = (deg.clamp(-max, max) * 1e7).round() as i32;
    v
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]
    use super::*;

    fn tile(layer: &str, class: &str, kind: i32, geometry: Vec<u32>) -> Vec<u8> {
        Tile {
            layers: vec![Layer {
                name: layer.into(),
                features: vec![Feature {
                    tags: vec![0, 0],
                    r#type: Some(kind),
                    geometry,
                }],
                keys: vec!["class".into()],
                values: vec![Value {
                    string_value: Some(class.into()),
                    int_value: None,
                    uint_value: None,
                }],
                extent: Some(4096),
            }],
        }
        .encode_to_vec()
    }

    #[test]
    fn a_road_and_a_lake_decode_in_tile_coordinates() {
        // MoveTo(1) (2048, 2048), LineTo(1) (+2048, 0).
        let road = tile(
            "transportation",
            "motorway",
            2,
            vec![9, 4096, 4096, 10, 4096, 0],
        );
        let s = shapes(&road, 10).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].class, Class::Motorway);
        assert_eq!(s[0].parts[0], vec![(0.5, 0.5), (1.0, 0.5)]);
        // A closed triangle.
        let lake = tile("water", "lake", 3, vec![9, 0, 0, 18, 4096, 0, 0, 4096, 15]);
        let s = shapes(&lake, 10).unwrap();
        assert_eq!(s[0].class, Class::Water);
        assert_eq!(s[0].parts[0].len(), 4);
        assert_eq!(s[0].parts[0][3], s[0].parts[0][0]);
    }

    #[test]
    fn hostile_tiles_are_refused_and_unknown_layers_skipped() {
        assert!(shapes(&[0xff; 64], 10).is_none());
        let bad_cmd = tile("water", "lake", 3, vec![4]);
        assert!(shapes(&bad_cmd, 10).is_none());
        let truncated = tile("water", "lake", 3, vec![9, 0]);
        assert!(shapes(&truncated, 10).is_none());
        let other = tile("poi", "cafe", 1, vec![9, 2, 2]);
        assert_eq!(shapes(&other, 10).unwrap().len(), 0);
    }

    #[test]
    fn projection_round_trips() {
        let (x, y) = project(46.77, 23.59, 12.0);
        let (lat, lon) = unproject(x, y, 12.0);
        assert!((lat - 46.77).abs() < 1e-9 && (lon - 23.59).abs() < 1e-9);
        assert_eq!(project(0.0, 0.0, 0.0), (0.5, 0.5));
        assert_eq!(e7(46.77, 90.0), 467_700_000);
        assert_eq!(e7(200.0, 180.0), 1_800_000_000);
    }
}
