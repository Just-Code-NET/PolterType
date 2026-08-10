# PolterType

Plattformübergreifender automatischer Tastaturlayout-Umschalter. Er lebt im
System-Tray, merkt, wenn du im falschen Layout zu tippen beginnst, schaltet das
Layout um und tippt das letzte Wort neu — wie ein freundlicher Poltergeist, der
in deiner Tastatur wohnt.

Die vollständige Dokumentation, die Entwicklungsnotizen und die ausführlichen
Vorbehalte stehen im [englischen README](README.md).

Bei der Entwicklung kam KI zum Einsatz. An die Qualität des Codes und
des fertigen Produkts legen wir strenge Maßstäbe an: Jede einzelne
Codezeile durchläuft ein Review.

## Installation

Die Binaries werden auf der [Releases-Seite](../../releases) veröffentlicht.
Jedes Release enthält vier Installer:

| System | Datei | Installation |
| --- | --- | --- |
| Windows 10 / 11 | `poltertype-<ver>-x86_64-pc-windows-msvc.msi` | Doppelklick. Installation pro Benutzer, ohne Administratorrechte und ohne UAC-Abfrage. SmartScreen zeigt eventuell „Der Computer wurde durch Windows geschützt“ → **Weitere Informationen** → **Trotzdem ausführen**. |
| macOS 11+ (Intel und Apple Silicon) | `poltertype-<ver>-universal-apple-darwin.dmg` | DMG öffnen und `poltertype.app` nach `/Applications` ziehen. Beim ersten Start Rechtsklick auf die App → **Öffnen** (oder `xattr -dr com.apple.quarantine /Applications/poltertype.app` ausführen). Danach **Bedienungshilfen** und **Eingabeüberwachung** erlauben, wenn macOS danach fragt. |
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
