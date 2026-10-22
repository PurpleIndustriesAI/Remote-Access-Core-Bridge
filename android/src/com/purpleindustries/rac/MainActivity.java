package com.purpleindustries.rac;

import android.Manifest;
import android.app.Activity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.provider.Settings;
import android.text.InputType;
import android.view.Gravity;
import android.view.View;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.Switch;
import android.widget.TextView;

import java.util.Collections;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Pair with a computer, send the clipboard, switch receiving on, and open the stream app. */
public final class MainActivity extends Activity {
    static final int BG = 0xFF0F1420, PANEL = 0xFF182033, TEXT = 0xFFE6EAF2, MUTED = 0xFF8A94A8, ACCENT = 0xFF3DDC97;
    static final String STREAM_APP = "com.limelight.fold";
    private final ExecutorService work = Executors.newSingleThreadExecutor();
    private final Handler ui = new Handler(Looper.getMainLooper());
    private LinearLayout root, peers;
    private TextView me, log;
    private Switch receive;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(new String[] {Manifest.permission.POST_NOTIFICATIONS}, 1);
        }
        root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        int pad = dp(18);
        root.setPadding(pad, pad, pad, pad);
        ScrollView scroll = new ScrollView(this);
        scroll.setBackgroundColor(BG);
        scroll.setFitsSystemWindows(true);
        scroll.addView(root);
        setContentView(scroll);

        TextView title = text("Remote Access Crew", 24, TEXT);
        title.setTypeface(Typeface.DEFAULT_BOLD);
        root.addView(title);
        me = text("", 13, MUTED);
        root.addView(me);

        LinearLayout send = panel("Copy and paste");
        send.addView(text("Copy something on the computer and it appears here while receiving is on. "
                + "To send from this phone, share any text, picture or file to Remote Access Crew, or:", 14, MUTED));
        send.addView(button("Send my clipboard to computers", v -> sendClipboard()));
        receive = new Switch(this);
        receive.setText("Receive from computers");
        receive.setTextColor(TEXT);
        receive.setTextSize(15);
        receive.setPadding(0, dp(10), 0, 0);
        receive.setOnCheckedChangeListener((b, on) -> {
            if (on) RacService.start(this); else RacService.stop(this);
        });
        send.addView(receive);

        LinearLayout stream = panel("Remote desktop");
        stream.addView(text("Streams the computer's screen with phone controls: Keys, zoom and pan, and the Windows key.", 14, MUTED));
        stream.addView(button("Open the stream", v -> openStream()));

        LinearLayout pair = panel("Pair a computer");
        pair.addView(text("On the computer run: rac pair-code\nThen enter its address and the code within 5 minutes.", 14, MUTED));
        EditText addr = field("Computer address, e.g. 192.168.0.159 or a Tailscale 100.x address", InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        EditText code = field("Pairing code XXXX-XXXX-XXXX-XXXX", InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);
        pair.addView(addr);
        pair.addView(code);
        pair.addView(button("Pair", v -> pair(addr.getText().toString(), code.getText().toString(), code)));

        peers = panel("Paired computers");

        LinearLayout activity = panel("Activity");
        log = text("", 12, MUTED);
        log.setTypeface(Typeface.MONOSPACE);
        activity.addView(log);

        Transfer.listener = () -> ui.post(this::refresh);
        work.execute(() -> {
            String fp;
            try {
                fp = Rac.shortFp(Rac.myFingerprint());
            } catch (Exception e) {
                fp = "unavailable (" + e.getMessage() + ")";
            }
            String shown = "This phone: " + deviceName() + "\nIdentity " + fp;
            ui.post(() -> me.setText(shown));
        });
        refresh();
    }

    @Override
    protected void onResume() {
        super.onResume();
        refresh();
    }

    @Override
    protected void onDestroy() {
        Transfer.listener = null;
        work.shutdown();
        super.onDestroy();
    }

    private void refresh() {
        receive.setOnCheckedChangeListener(null);
        receive.setChecked(RacService.running);
        receive.setOnCheckedChangeListener((b, on) -> {
            if (on) RacService.start(this); else RacService.stop(this);
        });
        while (peers.getChildCount() > 1) peers.removeViewAt(1);
        java.util.List<Rac.Peer> list = Rac.peers(this);
        if (list.isEmpty()) peers.addView(text("None yet.", 14, MUTED));
        for (Rac.Peer p : list) {
            LinearLayout row = new LinearLayout(this);
            row.setGravity(Gravity.CENTER_VERTICAL);
            TextView t = text(p.name + "\n" + Rac.shortFp(p.fingerprint) + "  " + (p.addrs.isEmpty() ? "" : p.addrs.get(0)), 14, TEXT);
            row.addView(t, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
            Button forget = new Button(this);
            forget.setText("Unpair");
            forget.setOnClickListener(v -> {
                Rac.forget(this, p.fingerprint);
                RacService.restartIfRunning(this);
                Transfer.log("Unpaired " + p.name);
            });
            row.addView(forget);
            peers.addView(row);
        }
        log.setText(Transfer.logText());
    }

    private void pair(String addr, String code, EditText codeField) {
        if (addr.trim().isEmpty()) {
            Transfer.log("Enter the computer's address first.");
            return;
        }
        Transfer.log("Pairing with " + addr.trim() + "...");
        work.execute(() -> {
            try {
                Rac.Peer p = Rac.pair(this, addr.trim(), code, deviceName());
                Transfer.log("Paired with " + p.name + " (" + Rac.shortFp(p.fingerprint) + "). Check it matches 'rac status' on the computer.");
                ui.post(() -> {
                    codeField.setText("");
                    RacService.restartIfRunning(this);
                    if (!RacService.running) RacService.start(this);
                });
            } catch (Exception e) {
                Transfer.log("Pairing failed: " + e.getMessage());
            }
        });
    }

    /** Android lets an app read the clipboard only while it is in the foreground, so this runs on a tap. */
    private void sendClipboard() {
        ClipboardManager cm = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
        ClipData c = cm.getPrimaryClip();
        if (c == null || c.getItemCount() == 0) {
            Transfer.log("The clipboard is empty.");
            return;
        }
        ClipData.Item item = c.getItemAt(0);
        Uri uri = item.getUri();
        String mime = c.getDescription().getMimeTypeCount() > 0 ? c.getDescription().getMimeType(0) : "";
        CharSequence txt = uri == null ? item.coerceToText(this) : null;
        work.execute(() -> {
            try {
                Transfer.Outgoing o;
                if (uri == null) o = Transfer.text(String.valueOf(txt));
                else if (mime.startsWith("image/")) o = Transfer.image(getContentResolver(), uri);
                else o = Transfer.files(getContentResolver(), Collections.singletonList(uri));
                Transfer.sendAll(this, o);
            } catch (Exception e) {
                Transfer.log("Could not send the clipboard: " + e.getMessage());
            }
        });
    }

    private void openStream() {
        Intent i = getPackageManager().getLaunchIntentForPackage(STREAM_APP);
        if (i == null) {
            Transfer.log("The stream app (Moonlight Fold) is not installed.");
            return;
        }
        startActivity(i);
    }

    private String deviceName() {
        String n = Settings.Global.getString(getContentResolver(), Settings.Global.DEVICE_NAME);
        return n == null || n.isEmpty() ? android.os.Build.MODEL : n;
    }

    // ------------------------------------------------------------------ views

    private int dp(int v) {
        return Math.round(v * getResources().getDisplayMetrics().density);
    }

    private TextView text(String s, int sp, int color) {
        TextView t = new TextView(this);
        t.setText(s);
        t.setTextSize(sp);
        t.setTextColor(color);
        t.setTextIsSelectable(true);
        return t;
    }

    private LinearLayout panel(String heading) {
        LinearLayout p = new LinearLayout(this);
        p.setOrientation(LinearLayout.VERTICAL);
        int pad = dp(14);
        p.setPadding(pad, pad, pad, pad);
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(PANEL);
        bg.setCornerRadius(dp(14));
        p.setBackground(bg);
        TextView h = text(heading, 16, ACCENT);
        h.setTypeface(Typeface.DEFAULT_BOLD);
        p.addView(h);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT);
        lp.topMargin = dp(14);
        root.addView(p, lp);
        return p;
    }

    private Button button(String label, View.OnClickListener l) {
        Button b = new Button(this);
        b.setText(label);
        b.setAllCaps(false);
        b.setTextColor(Color.BLACK);
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(ACCENT);
        bg.setCornerRadius(dp(10));
        b.setBackground(bg);
        b.setOnClickListener(l);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(48));
        lp.topMargin = dp(10);
        b.setLayoutParams(lp);
        return b;
    }

    private EditText field(String hint, int type) {
        EditText e = new EditText(this);
        e.setHint(hint);
        e.setHintTextColor(MUTED);
        e.setTextColor(TEXT);
        e.setInputType(type);
        e.setSingleLine(true);
        return e;
    }
}
