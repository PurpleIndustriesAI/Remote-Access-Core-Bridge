package com.purpleindustries.rac;

import android.content.Context;
import android.content.SharedPreferences;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.math.BigInteger;
import java.net.Inet4Address;
import java.net.Inet6Address;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.NetworkInterface;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.security.KeyPairGenerator;
import java.security.KeyStore;
import java.security.MessageDigest;
import java.security.Principal;
import java.security.PrivateKey;
import java.security.SecureRandom;
import java.security.cert.CertificateException;
import java.security.cert.X509Certificate;
import java.security.spec.ECGenParameterSpec;
import java.util.ArrayList;
import java.util.Collections;
import java.util.GregorianCalendar;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;

import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;
import javax.net.ssl.KeyManager;
import javax.net.ssl.SNIHostName;
import javax.net.ssl.SSLContext;
import javax.net.ssl.SSLParameters;
import javax.net.ssl.SSLServerSocket;
import javax.net.ssl.SSLSocket;
import javax.net.ssl.TrustManager;
import javax.net.ssl.X509KeyManager;
import javax.net.ssl.X509TrustManager;
import javax.security.auth.x500.X500Principal;

/**
 * The Remote Access Core (Bridge) wire protocol, compatible with the desktop bridge (src/proto.rs, src/tls.rs):
 * mutual TLS 1.3 with pinned certificate fingerprints, 4-byte big-endian length-prefixed JSON headers,
 * raw bodies, and an HMAC pairing proof that binds both fingerprints.
 */
final class Rac {
    static final int PORT = 48765;
    static final int MAX_HEADER = 256 * 1024;
    static final long MAX_TEXT = 8L << 20;
    static final long MAX_IMAGE = 128L << 20;
    static final long MAX_BATCH = 8L << 30;
    static final int MAX_FILES = 20_000;
    static final int CONNECT_TIMEOUT_MS = 3000;
    static final int IO_TIMEOUT_MS = 30_000;
    private static final String ALIAS = "remote-access-crew-identity";
    private static final String PREFS = "rac";

    private Rac() {}

    // ------------------------------------------------------------------ identity

    /** P-256 key in the Android Keystore. The private key never leaves the device's secure hardware. */
    static KeyStore.PrivateKeyEntry identity() throws Exception {
        KeyStore ks = KeyStore.getInstance("AndroidKeyStore");
        ks.load(null);
        if (!ks.containsAlias(ALIAS)) {
            KeyPairGenerator g = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, "AndroidKeyStore");
            g.initialize(new KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_SIGN)
                    .setAlgorithmParameterSpec(new ECGenParameterSpec("secp256r1"))
                    .setDigests(KeyProperties.DIGEST_SHA256, KeyProperties.DIGEST_NONE)
                    .setCertificateSubject(new X500Principal("CN=Remote Access Crew peer"))
                    .setCertificateSerialNumber(new BigInteger(63, new SecureRandom()).add(BigInteger.ONE))
                    .setCertificateNotBefore(new GregorianCalendar(2025, 0, 1).getTime())
                    .setCertificateNotAfter(new GregorianCalendar(2075, 0, 1).getTime())
                    .build());
            g.generateKeyPair();
        }
        return (KeyStore.PrivateKeyEntry) ks.getEntry(ALIAS, null);
    }

    static String fingerprint(byte[] certDer) throws Exception {
        return hex(MessageDigest.getInstance("SHA-256").digest(certDer));
    }

    static String myFingerprint() throws Exception {
        return fingerprint(identity().getCertificate().getEncoded());
    }

    /** Same grouping as the desktop's short_fingerprint, e.g. 50E7-F036-84B9-6821-4F2F-5242. */
    static String shortFp(String fp) {
        StringBuilder b = new StringBuilder();
        for (int i = 0; i < 24 && i < fp.length(); i += 4) {
            if (b.length() > 0) b.append('-');
            b.append(fp.substring(i, Math.min(i + 4, fp.length())).toUpperCase(Locale.ROOT));
        }
        return b.toString();
    }

    static String hex(byte[] b) {
        StringBuilder s = new StringBuilder(b.length * 2);
        for (byte x : b) s.append(String.format(Locale.ROOT, "%02x", x));
        return s.toString();
    }

    // ------------------------------------------------------------------ TLS

    /** Accepts only pinned fingerprints. With pins == null it accepts any certificate (pairing only) and records it. */
    static final class Pin implements X509TrustManager {
        final Set<String> pins;
        volatile String seen;

        Pin(Set<String> pins) {
            this.pins = pins;
        }

        private void check(X509Certificate[] chain) throws CertificateException {
            if (chain == null || chain.length == 0) throw new CertificateException("no certificate");
            try {
                seen = fingerprint(chain[0].getEncoded());
            } catch (Exception e) {
                throw new CertificateException(e);
            }
            if (pins != null && !pins.contains(seen)) throw new CertificateException("certificate is not paired");
        }

        @Override public void checkClientTrusted(X509Certificate[] chain, String auth) throws CertificateException { check(chain); }
        @Override public void checkServerTrusted(X509Certificate[] chain, String auth) throws CertificateException { check(chain); }
        @Override public X509Certificate[] getAcceptedIssuers() { return new X509Certificate[0]; }
    }

    private static final class Ident implements X509KeyManager {
        final PrivateKey key;
        final X509Certificate cert;

        Ident(KeyStore.PrivateKeyEntry e) {
            key = e.getPrivateKey();
            cert = (X509Certificate) e.getCertificate();
        }

        @Override public String[] getClientAliases(String t, Principal[] i) { return new String[] {ALIAS}; }
        @Override public String chooseClientAlias(String[] t, Principal[] i, Socket s) { return ALIAS; }
        @Override public String[] getServerAliases(String t, Principal[] i) { return new String[] {ALIAS}; }
        @Override public String chooseServerAlias(String t, Principal[] i, Socket s) { return ALIAS; }
        @Override public X509Certificate[] getCertificateChain(String a) { return new X509Certificate[] {cert}; }
        @Override public PrivateKey getPrivateKey(String a) { return key; }
    }

    static SSLContext context(Pin pin) throws Exception {
        SSLContext c = SSLContext.getInstance("TLSv1.3");
        c.init(new KeyManager[] {new Ident(identity())}, new TrustManager[] {pin}, new SecureRandom());
        return c;
    }

    static SSLServerSocket listen(Set<String> pins) throws Exception {
        SSLServerSocket s = (SSLServerSocket) context(new Pin(pins)).getServerSocketFactory().createServerSocket();
        s.setReuseAddress(true);
        s.bind(new InetSocketAddress(PORT), 16);
        s.setEnabledProtocols(new String[] {"TLSv1.3"});
        s.setNeedClientAuth(true);
        return s;
    }

    /** Connects with mutual TLS. The handshake fails unless the server's certificate matches the pin. */
    static SSLSocket dial(String addr, Pin pin) throws Exception {
        InetSocketAddress sa = parseAddr(addr);
        if (sa.isUnresolved() || !allowed(sa.getAddress())) throw new IOException(addr + " is not on your LAN or Tailscale network");
        Socket raw = new Socket();
        try {
            raw.connect(sa, CONNECT_TIMEOUT_MS);
            raw.setTcpNoDelay(true);
            raw.setSoTimeout(IO_TIMEOUT_MS);
            SSLSocket t = (SSLSocket) context(pin).getSocketFactory().createSocket(raw, "rac.peer", sa.getPort(), true);
            t.setEnabledProtocols(new String[] {"TLSv1.3"});
            SSLParameters p = t.getSSLParameters();
            p.setServerNames(Collections.singletonList(new SNIHostName("rac.peer")));
            t.setSSLParameters(p);
            t.startHandshake();
            return t;
        } catch (Exception e) {
            raw.close();
            throw e;
        }
    }

    static InetSocketAddress parseAddr(String a) throws IOException {
        a = a.trim();
        String host = a;
        int port = PORT;
        if (a.startsWith("[")) {
            int end = a.indexOf(']');
            if (end < 0) throw new IOException("bad address " + a);
            host = a.substring(1, end);
            if (a.length() > end + 2 && a.charAt(end + 1) == ':') port = Integer.parseInt(a.substring(end + 2));
        } else if (a.indexOf(':') == a.lastIndexOf(':') && a.indexOf(':') > 0) {
            host = a.substring(0, a.indexOf(':'));
            port = Integer.parseInt(a.substring(a.indexOf(':') + 1));
        }
        if (port < 1 || port > 65535) throw new IOException("bad port in " + a);
        return new InetSocketAddress(InetAddress.getByName(host), port);
    }

    // ------------------------------------------------------------------ network policy (src/net.rs)

    static boolean tailscale(InetAddress ip) {
        byte[] b = ip.getAddress();
        if (ip instanceof Inet4Address) return (b[0] & 0xff) == 100 && (b[1] & 0xc0) == 64;
        return (b[0] & 0xff) == 0xfd && (b[1] & 0xff) == 0x7a && (b[2] & 0xff) == 0x11 && (b[3] & 0xff) == 0x5c
                && (b[4] & 0xff) == 0xa1 && (b[5] & 0xff) == 0xe0;
    }

    /** Only loopback, private LAN, link-local and Tailscale addresses, as on the desktop. */
    static boolean allowed(InetAddress ip) {
        if (ip == null) return false;
        if (ip.isLoopbackAddress() || ip.isLinkLocalAddress() || tailscale(ip)) return true;
        if (ip instanceof Inet4Address) return ip.isSiteLocalAddress();
        byte[] b = ip.getAddress();
        if (((Inet6Address) ip).isIPv4CompatibleAddress()) return false;
        return (b[0] & 0xfe) == 0xfc;
    }

    /** Addresses a paired computer can use to reach this phone. */
    static List<String> localAddrs() {
        List<String> out = new ArrayList<>();
        try {
            for (NetworkInterface i : Collections.list(NetworkInterface.getNetworkInterfaces())) {
                if (!i.isUp() || i.isLoopback()) continue;
                for (InetAddress a : Collections.list(i.getInetAddresses())) {
                    if (a.isLoopbackAddress() || a.isLinkLocalAddress() || !allowed(a)) continue;
                    String h = a.getHostAddress();
                    int pct = h.indexOf('%');
                    if (pct >= 0) h = h.substring(0, pct);
                    String s = a instanceof Inet6Address ? "[" + h + "]:" + PORT : h + ":" + PORT;
                    if (a instanceof Inet4Address) out.add(0, s); else out.add(s);
                }
            }
        } catch (Exception ignored) {
            // No interface list: the computer still records the address this phone paired from.
        }
        return out.size() > 8 ? out.subList(0, 8) : out;
    }

    // ------------------------------------------------------------------ framing (src/proto.rs)

    static void writeJson(OutputStream o, JSONObject j) throws IOException {
        byte[] b = j.toString().getBytes(StandardCharsets.UTF_8);
        if (b.length > MAX_HEADER) throw new IOException("header too large");
        DataOutputStream d = new DataOutputStream(o);
        d.writeInt(b.length);
        d.write(b);
        d.flush();
    }

    static JSONObject readJson(InputStream in) throws IOException {
        DataInputStream d = new DataInputStream(in);
        int n = d.readInt();
        if (n < 0 || n > MAX_HEADER) throw new IOException("header too large");
        byte[] b = new byte[n];
        d.readFully(b);
        try {
            return new JSONObject(new String(b, StandardCharsets.UTF_8));
        } catch (JSONException e) {
            throw new IOException("bad header", e);
        }
    }

    static JSONObject reply(boolean ok, String error) {
        JSONObject r = new JSONObject();
        try {
            r.put("ok", ok);
            if (error != null) r.put("error", error);
        } catch (JSONException ignored) {
            // Constant keys; cannot fail.
        }
        return r;
    }

    // ------------------------------------------------------------------ pairing (src/proto.rs pair_proof)

    static String normalizeCode(String code) {
        StringBuilder b = new StringBuilder();
        for (char c : code.toCharArray()) if (c < 128 && Character.isLetterOrDigit(c)) b.append(Character.toUpperCase(c));
        return b.toString();
    }

    static String pairProof(String code, String role, String serverFp, String clientFp) throws Exception {
        Mac m = Mac.getInstance("HmacSHA256");
        m.init(new SecretKeySpec(normalizeCode(code).getBytes(StandardCharsets.US_ASCII), "HmacSHA256"));
        m.update(("remote-access-crew/pair/v1/" + role + "/" + serverFp + "/" + clientFp).getBytes(StandardCharsets.UTF_8));
        return hex(m.doFinal());
    }

    static boolean sameProof(String a, String b) {
        return a != null && b != null && MessageDigest.isEqual(a.getBytes(StandardCharsets.US_ASCII), b.getBytes(StandardCharsets.US_ASCII));
    }

    static String cleanName(String n) {
        StringBuilder b = new StringBuilder();
        for (char c : n.toCharArray()) {
            if (b.length() >= 48) break;
            if ((c < 128 && Character.isLetterOrDigit(c)) || "-_ .".indexOf(c) >= 0) b.append(c);
        }
        String s = b.toString().trim();
        return s.isEmpty() ? "peer" : s;
    }

    /** Pairs with a computer that has a pairing window open (rac pair-code). Returns the stored peer. */
    static Peer pair(Context ctx, String target, String code, String myName) throws Exception {
        if (normalizeCode(code).length() != 16) throw new IOException("pairing codes have 16 letters and digits");
        Pin pin = new Pin(null);
        String myFp = myFingerprint();
        try (SSLSocket t = dial(target, pin)) {
            String serverFp = pin.seen;
            JSONObject msg = new JSONObject()
                    .put("t", "pair")
                    .put("proof", pairProof(code, "client", serverFp, myFp))
                    .put("name", cleanName(myName))
                    .put("addrs", new JSONArray(localAddrs()));
            writeJson(t.getOutputStream(), msg);
            JSONObject r = readJson(t.getInputStream());
            if (!r.optBoolean("ok")) throw new IOException("pairing refused: " + r.optString("error"));
            if (!sameProof(pairProof(code, "server", serverFp, myFp), r.optString("proof", null))) {
                throw new IOException("the computer could not prove it knows the code; pairing stopped");
            }
            Peer p = new Peer();
            p.name = cleanName(r.optString("name", "peer"));
            p.fingerprint = serverFp;
            InetSocketAddress first = parseAddr(target);
            p.addrs.add(first.getAddress() instanceof Inet6Address
                    ? "[" + first.getAddress().getHostAddress() + "]:" + first.getPort()
                    : first.getAddress().getHostAddress() + ":" + first.getPort());
            JSONArray more = r.optJSONArray("addrs");
            for (int i = 0; more != null && i < more.length() && p.addrs.size() < 8; i++) {
                String a = more.optString(i);
                try {
                    if (allowed(parseAddr(a).getAddress()) && !p.addrs.contains(a)) p.addrs.add(a);
                } catch (Exception ignored) {
                    // Skip an address the computer listed that is not usable here.
                }
            }
            savePeer(ctx, p);
            return p;
        }
    }

    // ------------------------------------------------------------------ peers

    static final class Peer {
        String name;
        String fingerprint;
        final List<String> addrs = new ArrayList<>();

        JSONObject json() throws JSONException {
            return new JSONObject().put("name", name).put("fingerprint", fingerprint).put("addrs", new JSONArray(addrs));
        }

        static Peer from(JSONObject j) {
            Peer p = new Peer();
            p.name = j.optString("name", "peer");
            p.fingerprint = j.optString("fingerprint");
            JSONArray a = j.optJSONArray("addrs");
            for (int i = 0; a != null && i < a.length(); i++) p.addrs.add(a.optString(i));
            return p;
        }
    }

    private static SharedPreferences prefs(Context ctx) {
        return ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    static synchronized List<Peer> peers(Context ctx) {
        List<Peer> out = new ArrayList<>();
        try {
            JSONArray a = new JSONArray(prefs(ctx).getString("peers", "[]"));
            for (int i = 0; i < a.length(); i++) {
                Peer p = Peer.from(a.getJSONObject(i));
                if (p.fingerprint.matches("[0-9a-f]{64}")) out.add(p);
            }
        } catch (JSONException ignored) {
            // Corrupt list: treat as no peers rather than trusting any of it.
        }
        return out;
    }

    static Set<String> pins(Context ctx) {
        Set<String> s = new HashSet<>();
        for (Peer p : peers(ctx)) s.add(p.fingerprint);
        return s;
    }

    static synchronized void savePeers(Context ctx, List<Peer> list) {
        JSONArray a = new JSONArray();
        try {
            for (Peer p : list) a.put(p.json());
        } catch (JSONException ignored) {
            // Strings only; cannot fail.
        }
        prefs(ctx).edit().putString("peers", a.toString()).apply();
    }

    static synchronized void savePeer(Context ctx, Peer p) {
        List<Peer> list = peers(ctx);
        for (int i = list.size() - 1; i >= 0; i--) if (list.get(i).fingerprint.equals(p.fingerprint)) list.remove(i);
        list.add(p);
        savePeers(ctx, list);
    }

    static synchronized void forget(Context ctx, String fp) {
        List<Peer> list = peers(ctx);
        for (int i = list.size() - 1; i >= 0; i--) if (list.get(i).fingerprint.equals(fp)) list.remove(i);
        savePeers(ctx, list);
    }

    /** Puts an address that just worked first, so the next send tries it first. */
    static synchronized void promote(Context ctx, String fp, String addr) {
        List<Peer> list = peers(ctx);
        for (Peer p : list) {
            if (p.fingerprint.equals(fp) && p.addrs.remove(addr)) p.addrs.add(0, addr);
        }
        savePeers(ctx, list);
    }

    /** Records the address a paired computer just connected from, as the desktop's learn_addr does. */
    static synchronized void learn(Context ctx, String fp, InetAddress ip) {
        String h = ip.getHostAddress();
        if (h.indexOf('%') >= 0) h = h.substring(0, h.indexOf('%'));
        String a = ip instanceof Inet6Address ? "[" + h + "]:" + PORT : h + ":" + PORT;
        List<Peer> list = peers(ctx);
        for (Peer p : list) {
            if (p.fingerprint.equals(fp) && !p.addrs.contains(a)) {
                p.addrs.add(0, a);
                while (p.addrs.size() > 8) p.addrs.remove(p.addrs.size() - 1);
            }
        }
        savePeers(ctx, list);
    }

    // ------------------------------------------------------------------ received file paths (src/proto.rs)

    private static final Set<String> RESERVED = new HashSet<>(java.util.Arrays.asList(
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
            "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"));

    static String safeSegment(String seg) {
        if (seg.isEmpty() || seg.equals(".") || seg.equals("..")) return null;
        StringBuilder b = new StringBuilder();
        for (char c : seg.toCharArray()) b.append(Character.isISOControl(c) || "<>:\"/\\|?*".indexOf(c) >= 0 ? '_' : c);
        String s = b.toString();
        while (s.endsWith(".") || s.endsWith(" ")) s = s.substring(0, s.length() - 1);
        s = s.replaceAll("^\\s+", "");
        if (s.isEmpty()) s = "file";
        if (RESERVED.contains(s.split("\\.", -1)[0].toUpperCase(Locale.ROOT))) s = "_" + s;
        if (s.codePointCount(0, s.length()) > 180) s = s.substring(0, s.offsetByCodePoints(0, 180));
        return s;
    }

    /** Validates a relative '/' path from a peer and returns its safe segments. */
    static List<String> safeRelative(String path) throws IOException {
        if (path.startsWith("/") || path.startsWith("\\") || path.contains("\\")
                || (path.length() > 1 && path.charAt(1) == ':')) {
            throw new IOException("absolute paths are not accepted");
        }
        List<String> out = new ArrayList<>();
        for (String seg : path.split("/", -1)) {
            String s = safeSegment(seg);
            if (s == null) throw new IOException("unsafe path segment");
            out.add(s);
        }
        if (out.isEmpty() || out.size() > 64) throw new IOException("bad path depth");
        return out;
    }
}
