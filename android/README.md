# Remote Access Core (Bridge) for Android

The phone end of the clipboard and file bridge. It speaks the same protocol as the desktop
bridge, so a phone pairs with a computer exactly like a second computer does.

## What it does

- **Pair:** on the computer run `rac pair-code`, then enter the computer's address and the code
  in the app within 5 minutes. Check that the identity the app shows matches `rac status`.
- **Phone to computer:** share any text, picture or file to **Send to computers**, or tap
  **Send my clipboard to computers**. Pictures arrive on the computer's clipboard as images;
  other files keep their original bytes.
- **Computer to phone:** switch on **Receive from computers**. Copied text goes to the phone's
  clipboard. Images are saved to `Pictures/Remote Access Crew` and copied. Files are saved to
  `Download/Remote Access Crew/<date>` and the first one is copied.
- **Remote desktop:** **Open the stream** starts Moonlight Fold (`com.limelight.fold`), the phone
  streaming client with Keys, zoom and pan, and Windows key controls. It is a separate,
  GPL-licensed app in its own repository; this app only launches it.
- **Connection sharing:** WiFi and LAN stays on by default. USB and connected accessories and
  Bluetooth start off. Both ends must allow the detected transport before every transfer, and
  unidentified transports fail closed. RAC uses the existing network link and never changes an
  adapter, tethering or radio setting.

## Security

- Mutual TLS 1.3. The phone's key is a P-256 key in the Android Keystore and never leaves it.
- Each side trusts only the certificate fingerprint pinned at pairing. Pairing uses the
  desktop's HMAC proof over both fingerprints, so a relay cannot complete it.
- Only loopback, private LAN, link-local and Tailscale addresses are used, as on the desktop.
- The phone listens on port 48765 only while **Receive** is on, with a notification showing.
  It never accepts pairing requests; pairing always starts from the phone.
- Received files are checked against their SHA-256 sums before they are published. Paths are
  sanitized the same way as on the desktop. Nothing received is opened or run.
- No analytics, no network access other than paired computers, no backup of app data.
- Android only lets an app read the clipboard while it is on screen, so phone-to-computer copy
  is a tap or a share, not automatic.

## Build and test

Needs JDK 17+, Android SDK platform 36 and build-tools 35 or later. No Gradle.

```
$env:RAC_ANDROID_KEYSTORE = '<outside the repo>\rac-android-release.jks'   # alias "rac"
$env:RAC_ANDROID_KEYPASS  = '<outside the repo>\rac-android-release.pass.txt'
pwsh android/build.ps1        # writes android/build/RemoteAccessCrew.apk and prints its SHA-256
pwsh android/test.ps1         # protocol checks on the desktop JVM
adb install -r android/build/RemoteAccessCrew.apk
```

The signing key and its password stay outside the repository. Keep the same key for every
release: Android refuses an update signed with a different one.
