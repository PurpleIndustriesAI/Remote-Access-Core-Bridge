package com.purpleindustries.rac;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.widget.Toast;

import java.util.ArrayList;
import java.util.List;

/** Share target: text, one picture, or any files from another app go to every paired computer. */
public final class ShareActivity extends Activity {
    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        Intent in = getIntent();
        String type = in.getType() == null ? "" : in.getType();
        List<Uri> uris = new ArrayList<>();
        if (Intent.ACTION_SEND_MULTIPLE.equals(in.getAction())) {
            ArrayList<Uri> list = in.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri.class);
            if (list != null) uris.addAll(list);
        } else {
            Uri u = in.getParcelableExtra(Intent.EXTRA_STREAM, Uri.class);
            if (u != null) uris.add(u);
        }
        String text = in.getStringExtra(Intent.EXTRA_TEXT);
        Context app = getApplicationContext();
        Toast.makeText(this, "Sending to your computers...", Toast.LENGTH_SHORT).show();
        // The read grant for shared URIs lasts while this task exists, so the stream is read before finishing.
        new Thread(() -> {
            String result;
            try {
                Transfer.Outgoing o;
                if (uris.isEmpty() && text != null) o = Transfer.text(text);
                else if (uris.size() == 1 && type.startsWith("image/")) o = Transfer.image(app.getContentResolver(), uris.get(0));
                else if (!uris.isEmpty()) o = Transfer.files(app.getContentResolver(), uris);
                else throw new IllegalArgumentException("nothing to send");
                int n = Transfer.sendAll(app, o);
                result = n > 0 ? "Sent " + o.what + " to " + n + " computer(s)" : "Not sent. Open Remote Access Core (Bridge) for details.";
            } catch (Exception e) {
                result = "Not sent: " + e.getMessage();
                Transfer.log(result);
            }
            String shown = result;
            runOnUiThread(() -> {
                Toast.makeText(app, shown, Toast.LENGTH_LONG).show();
                finish();
            });
        }).start();
    }
}
