package com.purpleindustries.rac;

import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.ContentResolver;
import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.net.Uri;
import android.os.Environment;
import android.provider.MediaStore;
import android.provider.OpenableColumns;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.text.SimpleDateFormat;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Date;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;

import javax.net.ssl.SSLSocket;

/** Sending to and receiving from paired computers, plus the short event log the app shows. */
final class Transfer {
    private static final int BUF = 256 * 1024;
    private static final String FOLDER = "Remote Access Crew";

    private Transfer() {}

    // ------------------------------------------------------------------ event log

    interface Listener {
        void changed();
    }

    private static final List<String> LOG = Collections.synchronizedList(new ArrayList<>());
    static volatile Listener listener;

    static void log(String line) {
        String t = new SimpleDateFormat("HH:mm:ss", Locale.ROOT).format(new Date());
        LOG.add(0, t + "  " + line);
        while (LOG.size() > 40) LOG.remove(LOG.size() - 1);
        Listener l = listener;
        if (l != null) l.changed();
    }

    static String logText() {
        synchronized (LOG) {
            return String.join("\n", LOG);
        }
    }

    // ------------------------------------------------------------------ outgoing

    /** One item to send: a header message and how to stream its body. */
    static final class Outgoing {
        JSONObject msg;
        byte[] bytes;
        List<Uri> uris;
        long size;
        String what;
    }

    static Outgoing text(String s) throws JSONException {
        Outgoing o = new Outgoing();
        o.bytes = s.getBytes(StandardCharsets.UTF_8);
        if (o.bytes.length > Rac.MAX_TEXT) throw new JSONException("text is larger than 8 MB");
        o.msg = new JSONObject().put("t", "text").put("len", o.bytes.length);
        o.size = o.bytes.length;
        o.what = "text";
        return o;
    }

    /** The desktop puts received images on its clipboard and accepts only PNG, so other formats are converted. */
    static Outgoing image(ContentResolver cr, Uri uri) throws Exception {
        byte[] raw;
        try (InputStream in = cr.openInputStream(uri)) {
            raw = readLimited(in, Rac.MAX_IMAGE);
        }
        boolean png = raw.length > 8 && (raw[0] & 0xff) == 0x89 && raw[1] == 'P' && raw[2] == 'N' && raw[3] == 'G';
        if (!png) {
            Bitmap b = BitmapFactory.decodeByteArray(raw, 0, raw.length);
            if (b == null) throw new IOException("not a readable image");
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            b.compress(Bitmap.CompressFormat.PNG, 100, out);
            b.recycle();
            raw = out.toByteArray();
            if (raw.length > Rac.MAX_IMAGE) throw new IOException("image is larger than 128 MB as PNG");
        }
        Outgoing o = new Outgoing();
        o.bytes = raw;
        o.msg = new JSONObject().put("t", "image").put("len", raw.length);
        o.size = raw.length;
        o.what = "image";
        return o;
    }

    /** Original bytes of each file, with lengths and SHA-256 sums the receiver checks. */
    static Outgoing files(ContentResolver cr, List<Uri> uris) throws Exception {
        JSONArray entries = new JSONArray();
        Set<String> names = new HashSet<>();
        long total = 0;
        for (Uri u : uris) {
            String name = Rac.safeSegment(displayName(cr, u));
            if (name == null) name = "file";
            String unique = name;
            for (int i = 2; !names.add(unique); i++) unique = "(" + i + ") " + name;
            MessageDigest h = MessageDigest.getInstance("SHA-256");
            long len = 0;
            byte[] buf = new byte[BUF];
            try (InputStream in = cr.openInputStream(u)) {
                if (in == null) throw new IOException("cannot open " + name);
                for (int n; (n = in.read(buf)) > 0; ) {
                    h.update(buf, 0, n);
                    len += n;
                }
            }
            total += len;
            if (total > Rac.MAX_BATCH) throw new IOException("files are larger than 8 GB together");
            entries.put(new JSONObject().put("path", unique).put("len", len).put("sha256", Rac.hex(h.digest())));
        }
        Outgoing o = new Outgoing();
        o.msg = new JSONObject().put("t", "files").put("files", entries);
        o.uris = uris;
        o.size = total;
        o.what = uris.size() == 1 ? "file" : uris.size() + " files";
        return o;
    }

    static String displayName(ContentResolver cr, Uri u) {
        try (Cursor c = cr.query(u, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (c != null && c.moveToFirst() && !c.isNull(0)) return c.getString(0);
        } catch (Exception ignored) {
            // Fall back to the last path segment.
        }
        String s = u.getLastPathSegment();
        return s == null ? "file" : s;
    }

    /** Sends to every paired computer. Returns how many accepted it. */
    static int sendAll(Context ctx, Outgoing o) {
        List<Rac.Peer> peers = Rac.peers(ctx);
        if (peers.isEmpty()) {
            log("No paired computer. Pair one first.");
            return 0;
        }
        int ok = 0;
        for (Rac.Peer p : peers) {
            try {
                deliver(ctx, p, o);
                ok++;
                log("Sent " + o.what + " to " + p.name);
            } catch (Exception e) {
                log("Could not send " + o.what + " to " + p.name + ": " + e.getMessage());
            }
        }
        return ok;
    }

    private static void deliver(Context ctx, Rac.Peer p, Outgoing o) throws Exception {
        Set<String> pin = Collections.singleton(p.fingerprint);
        Exception last = new IOException("no address known for " + p.name);
        for (String a : p.addrs) {
            SSLSocket t;
            try {
                t = Rac.dial(a, new Rac.Pin(pin));
            } catch (Exception e) {
                last = new IOException(a + ": " + e.getMessage(), e);
                continue;
            }
            try (SSLSocket s = t) {
                OutputStream out = s.getOutputStream();
                Rac.writeJson(out, o.msg);
                if (o.bytes != null) out.write(o.bytes);
                if (o.uris != null) {
                    ContentResolver cr = ctx.getContentResolver();
                    JSONArray files = o.msg.getJSONArray("files");
                    byte[] buf = new byte[BUF];
                    for (int i = 0; i < o.uris.size(); i++) {
                        long left = files.getJSONObject(i).getLong("len");
                        try (InputStream in = cr.openInputStream(o.uris.get(i))) {
                            while (left > 0) {
                                int n = in.read(buf, 0, (int) Math.min(buf.length, left));
                                if (n <= 0) throw new IOException("a file changed while it was being sent");
                                out.write(buf, 0, n);
                                left -= n;
                            }
                        }
                    }
                }
                out.flush();
                // Large transfers can take the receiver a while to verify, as on the desktop.
                s.setSoTimeout(Rac.IO_TIMEOUT_MS + (int) Math.min(Integer.MAX_VALUE - Rac.IO_TIMEOUT_MS, o.size / (5 * 1024)));
                JSONObject r = Rac.readJson(s.getInputStream());
                if (!r.optBoolean("ok")) throw new IOException(p.name + " declined: " + r.optString("error"));
                Rac.promote(ctx, p.fingerprint, a);
                return;
            }
        }
        throw last;
    }

    static byte[] readLimited(InputStream in, long max) throws IOException {
        if (in == null) throw new IOException("cannot open the item");
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        byte[] buf = new byte[BUF];
        for (int n; (n = in.read(buf)) > 0; ) {
            if (out.size() + (long) n > max) throw new IOException("item is too large");
            out.write(buf, 0, n);
        }
        return out.toByteArray();
    }

    // ------------------------------------------------------------------ incoming

    /** Handles one connection from a paired computer. The TLS layer has already checked its pin. */
    static void receive(Context ctx, SSLSocket s) throws Exception {
        s.setSoTimeout(Rac.IO_TIMEOUT_MS);
        String fp = Rac.fingerprint(s.getSession().getPeerCertificates()[0].getEncoded());
        String peer = "a computer";
        for (Rac.Peer p : Rac.peers(ctx)) if (p.fingerprint.equals(fp)) peer = p.name;
        JSONObject msg = Rac.readJson(s.getInputStream());
        String t = msg.optString("t");
        String err = null;
        try {
            switch (t) {
                case "ping":
                    break;
                case "text":
                    receiveText(ctx, s.getInputStream(), msg.getLong("len"), peer);
                    break;
                case "image":
                    receiveImage(ctx, s.getInputStream(), msg.getLong("len"), peer);
                    break;
                case "files":
                    receiveFiles(ctx, s, msg.getJSONArray("files"), peer);
                    break;
                case "pair":
                    err = "pair from the phone app instead";
                    break;
                default:
                    err = "not supported on the phone app";
            }
        } catch (Exception e) {
            err = e.getMessage() == null ? e.toString() : e.getMessage();
            log("Rejected " + t + " from " + peer + ": " + err);
        }
        Rac.learn(ctx, fp, s.getInetAddress());
        Rac.writeJson(s.getOutputStream(), Rac.reply(err == null, err));
    }

    private static byte[] body(InputStream in, long len, long max, String what) throws IOException {
        if (len < 0 || len > max) throw new IOException(what + " is larger than the phone's limit");
        byte[] b = new byte[(int) len];
        new DataInputStream(in).readFully(b);
        return b;
    }

    private static void receiveText(Context ctx, InputStream in, long len, String peer) throws IOException {
        byte[] b = body(in, len, Rac.MAX_TEXT, "text");
        String text = StandardCharsets.UTF_8.newDecoder().decode(java.nio.ByteBuffer.wrap(b)).toString();
        clip(ctx, ClipData.newPlainText("Remote Access Crew", text));
        log("Text from " + peer + " copied (" + text.length() + " characters)");
    }

    private static void receiveImage(Context ctx, InputStream in, long len, String peer) throws IOException {
        byte[] b = body(in, len, Rac.MAX_IMAGE, "image");
        // Validate before saving: a PNG signature and a decodable header.
        BitmapFactory.Options o = new BitmapFactory.Options();
        o.inJustDecodeBounds = true;
        BitmapFactory.decodeByteArray(b, 0, b.length, o);
        if (b.length < 8 || (b[0] & 0xff) != 0x89 || b[1] != 'P' || o.outWidth <= 0 || o.outHeight <= 0) throw new IOException("not a valid PNG image");
        String name = "Copied " + new SimpleDateFormat("yyyy-MM-dd HH.mm.ss", Locale.ROOT).format(new Date()) + ".png";
        ContentResolver cr = ctx.getContentResolver();
        ContentValues v = new ContentValues();
        v.put(MediaStore.Images.Media.DISPLAY_NAME, name);
        v.put(MediaStore.Images.Media.MIME_TYPE, "image/png");
        v.put(MediaStore.Images.Media.RELATIVE_PATH, Environment.DIRECTORY_PICTURES + "/" + FOLDER);
        v.put(MediaStore.Images.Media.IS_PENDING, 1);
        Uri u = cr.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, v);
        if (u == null) throw new IOException("cannot save the image");
        try (OutputStream out = cr.openOutputStream(u)) {
            out.write(b);
        } catch (IOException e) {
            cr.delete(u, null, null);
            throw e;
        }
        v.clear();
        v.put(MediaStore.Images.Media.IS_PENDING, 0);
        cr.update(u, v, null, null);
        clip(ctx, ClipData.newUri(cr, "Remote Access Crew image", u));
        log("Image from " + peer + " copied and saved to Pictures/" + FOLDER);
    }

    private static void receiveFiles(Context ctx, SSLSocket s, JSONArray files, String peer) throws Exception {
        int count = files.length();
        if (count == 0 || count > Rac.MAX_FILES) throw new IOException("bad file count");
        long total = 0;
        List<List<String>> paths = new ArrayList<>();
        Set<String> seen = new HashSet<>();
        for (int i = 0; i < count; i++) {
            JSONObject f = files.getJSONObject(i);
            long len = f.getLong("len");
            if (len < 0) throw new IOException("bad file length");
            total += len;
            if (total > Rac.MAX_BATCH) throw new IOException("files are larger than the phone's limit");
            if (!f.getString("sha256").matches("[0-9a-fA-F]{64}")) throw new IOException("bad checksum format");
            List<String> segs = Rac.safeRelative(f.getString("path"));
            if (!seen.add(String.join("/", segs))) throw new IOException("duplicate file path in batch");
            paths.add(segs);
        }
        // Validate everything before writing, then write each file pending and publish it only once its hash matches.
        String batch = new SimpleDateFormat("yyyy-MM-dd HH.mm.ss", Locale.ROOT).format(new Date());
        ContentResolver cr = ctx.getContentResolver();
        List<Uri> written = new ArrayList<>();
        s.setSoTimeout(Rac.IO_TIMEOUT_MS * 4);
        InputStream in = s.getInputStream();
        byte[] buf = new byte[BUF];
        try {
            for (int i = 0; i < count; i++) {
                JSONObject f = files.getJSONObject(i);
                List<String> segs = paths.get(i);
                StringBuilder rel = new StringBuilder(Environment.DIRECTORY_DOWNLOADS + "/" + FOLDER + "/" + batch);
                for (int k = 0; k < segs.size() - 1; k++) rel.append('/').append(segs.get(k));
                ContentValues v = new ContentValues();
                v.put(MediaStore.Downloads.DISPLAY_NAME, segs.get(segs.size() - 1));
                v.put(MediaStore.Downloads.RELATIVE_PATH, rel.toString());
                v.put(MediaStore.Downloads.IS_PENDING, 1);
                Uri u = cr.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, v);
                if (u == null) throw new IOException("cannot save " + segs.get(segs.size() - 1));
                written.add(u);
                MessageDigest h = MessageDigest.getInstance("SHA-256");
                long left = f.getLong("len");
                try (OutputStream out = cr.openOutputStream(u)) {
                    while (left > 0) {
                        int n = in.read(buf, 0, (int) Math.min(buf.length, left));
                        if (n <= 0) throw new IOException("the computer stopped sending");
                        h.update(buf, 0, n);
                        out.write(buf, 0, n);
                        left -= n;
                    }
                }
                if (!Rac.hex(h.digest()).equalsIgnoreCase(f.getString("sha256"))) throw new IOException("checksum mismatch; nothing was kept");
            }
        } catch (Exception e) {
            for (Uri u : written) cr.delete(u, null, null);
            throw e;
        }
        ContentValues done = new ContentValues();
        done.put(MediaStore.Downloads.IS_PENDING, 0);
        for (Uri u : written) cr.update(u, done, null, null);
        clip(ctx, ClipData.newUri(cr, "Remote Access Crew file", written.get(0)));
        log((count == 1 ? "File" : count + " files") + " from " + peer + " saved to Download/" + FOLDER + "/" + batch);
    }

    private static void clip(Context ctx, ClipData c) {
        ClipboardManager cm = (ClipboardManager) ctx.getSystemService(Context.CLIPBOARD_SERVICE);
        cm.setPrimaryClip(c);
    }
}
