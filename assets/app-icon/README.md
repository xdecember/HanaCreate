# HanaCreate app icon

The Android adaptive icon and desktop window icon are generated from the supplied master logo at `~/HanaCreate/Resources/HanaCreateDesign/HanaCreate0.0.1Icon.jpg`.

## Files

| File | Purpose |
|---|---|
| `hanacreate-256.png`, `hanacreate-512.png`, `hanacreate-1024.png`, `hanacreate.ico` | Desktop and listing icon assets |
| `android/app/src/main/res/drawable-*dpi/ic_launcher_foreground.png` | Android adaptive icon foreground |
| `android/app/src/main/res/drawable-*dpi/ic_launcher_monochrome.png` | Android themed icon foreground |
| `android/app/src/main/res/drawable-nodpi/hanacreate_mark.png` | About-screen and desktop gallery mark |

## Regenerate

```sh
python3 tools/app-icon/make_hanacreate_icon.py
```
