# PolterType

Plattformübergreifender automatischer Tastaturlayout-Umschalter. Er lebt im
System-Tray, merkt, wenn du im falschen Layout zu tippen beginnst, schaltet das
Layout um und tippt das letzte Wort neu — wie ein freundlicher Poltergeist, der
in deiner Tastatur wohnt.

Die vollständige Dokumentation, die Entwicklungsnotizen und die ausführlichen
Vorbehalte stehen im [englischen README](README.md).

Bei der Entwicklung kam KI zum Einsatz.

## Installation

Die Binaries werden auf der [Releases-Seite](../../releases) veröffentlicht.
Jedes Release enthält vier Installer:

| System | Datei | Installation |
| --- | --- | --- |
| Windows 10 / 11 | `poltertype-<ver>-x86_64-pc-windows-msvc.msi` | Doppelklick. Installation pro Benutzer, ohne Administratorrechte und ohne UAC-Abfrage. SmartScreen zeigt eventuell „Der Computer wurde durch Windows geschützt“ → **Weitere Informationen** → **Trotzdem ausführen**. |
| macOS 11+ (Intel und Apple Silicon) | `poltertype-<ver>-universal-apple-darwin.dmg` | DMG öffnen und `poltertype.app` nach `/Applications` ziehen. Beim ersten Start Rechtsklick auf die App → **Öffnen** (oder `xattr -dr com.apple.quarantine /Applications/poltertype.app` ausführen). Danach **Bedienungshilfen** und **Eingabeüberwachung** erlauben, wenn macOS danach fragt. Nach jedem Update erneut erlauben: ohne Developer ID erkennt macOS die App am Hash ihrer eigenen Bytes, eine neue Version ist für das System also eine andere Software. **Auf Intel 0.14.4 oder neuer nehmen:** in allen früheren DMGs war der x86_64-Teil unsigniert, und unsigniertem Code gewährt macOS keine Bedienungshilfen — die App lief, die Berechtigung sah erteilt aus, korrigiert wurde nie etwas ([#28](https://github.com/Just-Code-NET/PolterType/issues/28)). |
| Linux (x86_64) | `poltertype-<ver>-x86_64.AppImage` | `chmod +x` und starten. Pro Benutzer, keine Systeminstallation. Zum `evdev`-Zugriff unter Wayland siehe [docs/PERMISSIONS.md](docs/PERMISSIONS.md). |
| Linux (aarch64) | `poltertype-<ver>-aarch64.AppImage` | Wie oben, für ARM64: Raspberry Pi 5, Asahi, ARM-Notebooks und -Server. |

> Die Installer sind weiterhin **nicht signiert**, deshalb warnen Gatekeeper
> bzw. SmartScreen beim ersten Start. Die Code-Signierung kommt in einer
> späteren Phase.

> **Es gibt kein Flatpak, und es wird keines geben.** PolterType schreibt nach
> `/dev/uinput`, wozu keine Flatpak-Berechtigung das Recht gibt außer
> `--device=all` (dem gesamten Gerätebaum), und ein Portal dafür existiert
> nicht. Das Umschalten des Layouts braucht außerdem Binaries des Systems
> (`hyprctl`, `gsettings`, `gdbus`, `qdbus`, `ibus`), die eine Sandbox nicht
> hat. Die Begründung, die Quellen und die Bedingungen, unter denen wir das
> neu bewerten würden, stehen in [docs/DECISIONS.md](docs/DECISIONS.md)
> (2026-07-31). Nimm das AppImage oder ein natives Paket.

Das Bauen aus dem Quellcode ist in [CONTRIBUTING.md](CONTRIBUTING.md)
dokumentiert.
