## Download

### Windows
| Package | For |
| --- | --- |
| [Harness.Studio_{VERSION}_Windows_x64-setup.exe]({BASE}/Harness.Studio_{VERSION}_Windows_x64-setup.exe) | Windows 10 / 11, 64-bit |

The installer is not code-signed yet. If SmartScreen warns you, choose **More info**, then **Run anyway**.

### macOS (13.3 or later)
| Package | For |
| --- | --- |
| [Harness.Studio_{VERSION}_macOS_Apple-Silicon.dmg]({BASE}/Harness.Studio_{VERSION}_macOS_Apple-Silicon.dmg) | Apple Silicon: M1, M2, M3, M4 |
| [Harness.Studio_{VERSION}_macOS_Intel.dmg]({BASE}/Harness.Studio_{VERSION}_macOS_Intel.dmg) | Intel Macs |

Open the DMG and drag Harness Studio to Applications. The app is signed with a Developer ID and notarized by Apple.

### Linux (preview)
| Package | For |
| --- | --- |
| [Harness.Studio_{VERSION}_Linux_x86_64.AppImage]({BASE}/Harness.Studio_{VERSION}_Linux_x86_64.AppImage) | Most distributions, x86_64; updates itself |
| [Harness.Studio_{VERSION}_Linux_x86_64.deb]({BASE}/Harness.Studio_{VERSION}_Linux_x86_64.deb) | Ubuntu / Debian, x86_64 |

AppImage: make it executable (`chmod +x Harness.Studio_{VERSION}_Linux_x86_64.AppImage`) and run it. It does not need `libfuse2`; where FUSE is not available (some containers), run it with `--appimage-extract-and-run`.
.deb: install with `sudo apt install ./Harness.Studio_{VERSION}_Linux_x86_64.deb`.

---

`latest.json`, the `.sig` files and the `.app.tar.gz` archives are used by the in-app updater; you do not need to download them.

See the [changelog](https://github.com/struklabs/harness-studio/blob/main/docs/CHANGELOG.md) for what is new.
