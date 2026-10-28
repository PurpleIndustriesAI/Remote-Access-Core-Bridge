package com.purpleindustries.rac;

import java.net.InetAddress;

/**
 * Desktop-JVM checks of the protocol parts that must match src/proto.rs and src/net.rs exactly.
 * Run: android/test.ps1 (no phone or emulator needed).
 */
public final class RacTest {
    private static int failed;

    private static void check(String name, boolean ok) {
        System.out.println((ok ? "PASS " : "FAIL ") + name);
        if (!ok) failed++;
    }

    private static boolean rejects(String path) {
        try {
            Rac.safeRelative(path);
            return false;
        } catch (Exception e) {
            return true;
        }
    }

    private static boolean ok(String ip) throws Exception {
        return Rac.allowed(InetAddress.getByName(ip));
    }

    public static void main(String[] args) throws Exception {
        // Same vector as proto::tests::proof_known_vector.
        check("pairing proof matches the desktop", Rac.pairProof("abcd-efgh-2345-6789", "client", "aa".repeat(32), "bb".repeat(32))
                .equals("ffd48229226b9d27dd14b831a871581a1e8227a323da0886f289ebd95047abec"));
        check("proof binds the role", !Rac.pairProof("ABCDEFGH23456789", "server", "aa", "bb").equals(Rac.pairProof("ABCDEFGH23456789", "client", "aa", "bb")));
        check("code normalization ignores dashes and case", Rac.normalizeCode("abcd-efgh-2345-6789").equals("ABCDEFGH23456789"));

        boolean all = true;
        for (String p : new String[] {"../x", "a/../../x", "/etc/passwd", "C:/Windows/x", "a\\b", "", "a//b", "./a"}) all &= rejects(p);
        check("traversal and absolute paths are rejected", all);
        check("names are sanitized like the desktop", String.join("/", Rac.safeRelative("dir/re<po>rt?.txt")).equals("dir/re_po_rt_.txt")
                && Rac.safeSegment("CON.txt").equals("_CON.txt") && Rac.safeSegment("evil. . ").equals("evil") && Rac.safeSegment("a\u0000b").equals("a_b"));

        check("LAN, Tailscale and loopback are allowed", ok("192.168.0.159") && ok("10.1.2.3") && ok("172.16.0.1") && ok("100.99.1.2")
                && ok("127.0.0.1") && ok("fd7a:115c:a1e0::1") && ok("fe80::1") && ok("fd00::1"));
        check("public addresses are refused", !ok("8.8.8.8") && !ok("100.128.0.1") && !ok("2001:4860::8888") && !ok("1.1.1.1"));
        check("transport names match the desktop", Rac.transportName("rndis0").equals("usb")
                && Rac.transportName("Bluetooth Network Connection").equals("bluetooth")
                && Rac.transportName("Ethernet 2").equals("wifi"));
        check("unidentified adapters are refused", Rac.transportName("mystery0").equals("unknown")
                && Rac.transportName("tun0").equals("unknown"));

        check("short fingerprint matches the desktop format", Rac.shortFp("50e7f03684b968214f2f5242aaaa").equals("50E7-F036-84B9-6821-4F2F-5242"));
        check("peer names are cleaned like the desktop", Rac.cleanName("  Fold<script>2  ").equals("Foldscript2") && Rac.cleanName("***").equals("peer"));
        check("addresses parse with and without a port", Rac.parseAddr("192.168.0.159").getPort() == Rac.PORT
                && Rac.parseAddr("100.99.1.2:5000").getPort() == 5000 && Rac.parseAddr("[fd7a:115c:a1e0::1]:48765").getPort() == 48765);

        System.out.println(failed + " failed");
        System.exit(failed == 0 ? 0 : 1);
    }
}
