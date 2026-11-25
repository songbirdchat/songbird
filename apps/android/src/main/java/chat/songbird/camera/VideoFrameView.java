package chat.songbird.camera;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Matrix;
import android.graphics.Paint;
import android.view.View;

import java.nio.ByteBuffer;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Map;
import java.util.Set;

/**
 * The native component {@code songbird/video} (D-0183): another participant's call video, the
 * feed its {@code feed} prop names, drawn from a reused bitmap (aspect fill). Frames come from
 * Rust's decoder thread ({@link CameraBridge#videoFrame}), not through the UI engine. Made and
 * driven by the Rust component the camera crate registers with NSH (D-0188); UI thread.
 */
public final class VideoFrameView {
    private static final Object LOCK = new Object();
    /** Each feed's latest frame, and the views showing it. */
    private static final Map<Long, Bitmap> FRAMES = new HashMap<>();
    private static final Map<Long, Set<Surface>> VIEWS = new HashMap<>();

    private final Surface view;
    private long feed = -1;

    public VideoFrameView(Context context) {
        view = new Surface(context);
    }

    public View view() {
        return view;
    }

    /** The {@code feed} prop; less than 0 shows none. */
    public void setFeed(long id) {
        unregister();
        if (id < 0) {
            view.postInvalidate();
            return;
        }
        feed = id;
        synchronized (LOCK) {
            VIEWS.computeIfAbsent(feed, k -> new HashSet<>()).add(view);
        }
        view.feed = feed;
        view.postInvalidate();
    }

    public void destroy() {
        unregister();
    }

    private void unregister() {
        if (feed < 0) return;
        synchronized (LOCK) {
            Set<Surface> set = VIEWS.get(feed);
            if (set != null) {
                set.remove(view);
                if (set.isEmpty()) VIEWS.remove(feed);
            }
        }
        feed = -1;
        view.feed = -1;
    }

    /** A frame from Rust: copied into the feed's bitmap now (the buffer is Rust's). */
    static void frame(long feed, int width, int height, ByteBuffer rgba) {
        Set<Surface> views;
        synchronized (LOCK) {
            Bitmap b = FRAMES.get(feed);
            if (b == null || b.getWidth() != width || b.getHeight() != height) {
                b = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888);
                FRAMES.put(feed, b);
            }
            rgba.rewind();
            b.copyPixelsFromBuffer(rgba);
            Set<Surface> set = VIEWS.get(feed);
            views = set == null ? null : new HashSet<>(set);
        }
        if (views != null) {
            for (Surface v : views) v.postInvalidate();
        }
    }

    static void clear(long feed) {
        Set<Surface> views;
        synchronized (LOCK) {
            FRAMES.remove(feed);
            Set<Surface> set = VIEWS.get(feed);
            views = set == null ? null : new HashSet<>(set);
        }
        if (views != null) {
            for (Surface v : views) v.postInvalidate();
        }
    }

    /** The view: draws its feed's bitmap, cropped to fill. */
    static final class Surface extends View {
        long feed = -1;
        private final Paint paint = new Paint(Paint.FILTER_BITMAP_FLAG);
        private final Matrix matrix = new Matrix();

        Surface(Context c) {
            super(c);
        }

        @Override
        protected void onDraw(Canvas canvas) {
            synchronized (LOCK) {
                Bitmap b = FRAMES.get(feed);
                if (b == null) return;
                float scale = Math.max((float) getWidth() / b.getWidth(), (float) getHeight() / b.getHeight());
                matrix.setScale(scale, scale);
                matrix.postTranslate((getWidth() - b.getWidth() * scale) / 2f, (getHeight() - b.getHeight() * scale) / 2f);
                canvas.drawBitmap(b, matrix, paint);
            }
        }
    }
}
