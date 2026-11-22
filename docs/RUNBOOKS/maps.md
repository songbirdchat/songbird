# Map tiles (D-0142)

Songbird serves OpenStreetMap vector tiles from its own storage (`GET /v1/maps/v1/<z>/<x>/<y>`),
so no map company learns where users look. Until the tiles are loaded, maps in the app show only
the background and the attribution.

## Build and load (once, then about yearly)

1. Build the tiles with [Planetiler](https://github.com/onthegomap/planetiler) (Java 21, about
   64 GB of RAM for Europe; a few hours):

   ```bash
   java -Xmx60g -jar planetiler.jar --download --area=europe --output=europe.mbtiles --maxzoom=14
   ```

   The output uses the OpenMapTiles schema, which `songbird_app_core::maps` styles.
2. Load it into the media bucket the api serves from (the same RustFS credentials as `api`):

   ```bash
   SONGBIRD_S3_ACCESS_KEY=… SONGBIRD_S3_SECRET_KEY=… \
     cargo run --release -p songbird-maps-import -- \
     --mbtiles europe.mbtiles --s3 <rustfs host:port> --bucket <api media bucket>
   ```

   Europe at z0–14 is roughly 30 to 60 million tiles and 40 to 70 GB.
3. Check: `curl -s -o /dev/null -w '%{http_code}' https://api.<domain>/v1/maps/v1/0/0/0` answers
   `200`.

## Licence

The data is © OpenStreetMap contributors under the ODbL. The app shows the attribution on every
map. Keep it if the style changes.

## Privacy

Tile requests are unsigned and name no account. The server sees which areas an IP address asks
for (D-0142, threat-model row proposed). Keep the api's access logs off for `/v1/maps/` (as for
every Songbird route, invariant 6).
