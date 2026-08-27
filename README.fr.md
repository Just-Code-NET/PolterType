# <img src="docs/icon.png" width="32" height="32" align="absmiddle" alt=""> PolterType

Commutateur automatique de disposition clavier, multiplateforme. Il vit dans la
zone de notification, repère le moment où vous commencez à taper dans la
mauvaise disposition, change de disposition et retape le dernier mot — comme un
poltergeist bienveillant qui hante votre clavier.

Pour la documentation complète, les notes de développement et les mises en
garde détaillées, voir le [README en anglais](README.md).

L'IA a été mise à contribution durant le développement. Nous sommes
très exigeants sur la qualité du code comme sur celle du produit fini :
chaque ligne de code fait l'objet d'une revue.

## Installation

Les binaires sont publiés sur la [page Releases](../../releases). Chaque
version contient quatre installateurs :

| Système | Fichier | Comment installer |
| --- | --- | --- |
| Windows 10 / 11 | `poltertype-<ver>-x86_64-pc-windows-msvc.msi` | Double-clic. Installation par utilisateur, sans droits d'administrateur ni invite UAC. SmartScreen peut afficher « Windows a protégé votre ordinateur » → **Informations complémentaires** → **Exécuter quand même**. |
| macOS 11+ (Intel et Apple Silicon) | `poltertype-<ver>-universal-apple-darwin.dmg` | Ouvrez le DMG et glissez `poltertype.app` dans `/Applications`. Au premier lancement, faites un clic droit sur l'application → **Ouvrir** (ou exécutez `xattr -dr com.apple.quarantine /Applications/poltertype.app`). Accordez ensuite **Accessibilité** et **Surveillance de la saisie** quand macOS le demande. Il faudra les réaccorder après chaque mise à jour : sans Developer ID, macOS identifie l'application par le hachage de ses propres octets, donc une nouvelle version est pour lui un autre logiciel. **Sur Intel, prenez 0.14.4 ou plus récent :** dans les DMG précédents, la tranche x86_64 n'était pas signée, et macOS n'accorde pas l'Accessibilité à du code non signé — l'application tournait, la permission semblait accordée, et rien n'était jamais corrigé ([#28](https://github.com/Just-Code-NET/PolterType/issues/28)). |
| Linux (x86_64) | `poltertype-<ver>-x86_64.AppImage` | `chmod +x` puis lancez-le. Par utilisateur, sans installation système. Pour l'accès `evdev` sous Wayland, voir [docs/PERMISSIONS.md](docs/PERMISSIONS.md) — également pour NixOS, où un AppImage ne démarre pas sans `programs.appimage.binfmt`. |
| Linux (aarch64) | `poltertype-<ver>-aarch64.AppImage` | Comme ci-dessus, pour ARM64 : Raspberry Pi 5, Asahi, portables et serveurs ARM. |

> Les installateurs ne sont **toujours pas signés**, c'est pourquoi Gatekeeper
> ou SmartScreen avertissent au premier lancement. La signature de code viendra
> dans une phase ultérieure.

> **Il n'y a pas de Flatpak, et il n'y en aura pas.** PolterType écrit dans
> `/dev/uinput`, ce qu'aucune permission Flatpak n'autorise en dehors de
> `--device=all` (tout l'arbre des périphériques), et il n'existe pas de portail
> pour cela. Le changement de disposition a aussi besoin de binaires du système
> (`hyprctl`, `gsettings`, `gdbus`, `qdbus`, `ibus`) qu'un bac à sable n'a pas.
> Le raisonnement, les sources et les conditions dans lesquelles nous
> reconsidérerions la question sont dans
> [docs/DECISIONS.md](docs/DECISIONS.md) (2026-07-31). Utilisez l'AppImage, ou
> un paquet natif.

La compilation depuis les sources est documentée dans
[CONTRIBUTING.md](CONTRIBUTING.md).
