# <img src="docs/icon.png" width="32" height="32" align="absmiddle" alt=""> PolterType

Cambiador automático de distribución de teclado multiplataforma. Vive en la
bandeja del sistema, detecta cuando empiezas a escribir en la distribución
incorrecta, cambia de distribución y vuelve a escribir la última palabra, como
un poltergeist amable que habita tu teclado.

Para la documentación completa, las notas de desarrollo y las advertencias
detalladas, consulta el [README en inglés](README.md).

En el proceso de desarrollo se recurrió a la IA. Somos muy exigentes
con la calidad del código y con la del producto final: cada línea de
código pasa por revisión.

## Instalación

Los binarios se publican en la [página de Releases](../../releases). Cada
release incluye cuatro instaladores:

| Sistema | Archivo | Cómo instalar |
| --- | --- | --- |
| Windows 10 / 11 | `poltertype-<ver>-x86_64-pc-windows-msvc.msi` | Doble clic. Instalación por usuario, sin derechos de administrador ni UAC. SmartScreen puede mostrar "Windows protegió tu PC" → **Más información** → **Ejecutar de todas formas**. |
| macOS 11+ (Intel y Apple Silicon) | `poltertype-<ver>-universal-apple-darwin.dmg` | Abre el DMG y arrastra `poltertype.app` a `/Applications`. En el primer lanzamiento, haz clic derecho sobre la app → **Abrir** (o ejecuta `xattr -dr com.apple.quarantine /Applications/poltertype.app`). Luego concede **Accesibilidad** y **Monitoreo de entrada** cuando macOS lo solicite. Habrá que concederlos de nuevo tras cada actualización: sin un Developer ID, macOS identifica la app por el hash de sus propios bytes, así que una versión nueva es, para el sistema, otro programa. **En Intel, usa 0.14.4 o posterior:** en los DMG anteriores la porción x86_64 iba sin firmar, y macOS no concede Accesibilidad a código sin firma — la app se ejecutaba, el permiso figuraba como concedido y no se corregía nada ([#28](https://github.com/Just-Code-NET/PolterType/issues/28)). |
| Linux (x86_64) | `poltertype-<ver>-x86_64.AppImage` | `chmod +x` y ejecuta. Instalación por usuario, sin instalación del sistema. Consulta [docs/PERMISSIONS.md](docs/PERMISSIONS.md) para el acceso `evdev` en Wayland y para NixOS, donde un AppImage no arranca sin `programs.appimage.binfmt`. |
| Linux (aarch64) | `poltertype-<ver>-aarch64.AppImage` | Igual que arriba, para ARM64: Raspberry Pi 5, Asahi, laptops y servidores ARM. |

> Los instaladores todavía **no están firmados**, por eso Gatekeeper o
> SmartScreen advierten en el primer lanzamiento. La firma de código llegará en
> una fase posterior.

> **No hay Flatpak y no lo habrá.** PolterType escribe en `/dev/uinput`, lo que
> ningún permiso de Flatpak concede salvo `--device=all` (todo el árbol de
> dispositivos), y no existe un portal para ello. El cambio de distribución
> también necesita binarios del sistema (`hyprctl`, `gsettings`, `gdbus`,
> `qdbus`, `ibus`) que una sandbox no tiene. El razonamiento, las fuentes y las
> condiciones bajo las cuales lo reconsideraríamos están en
> [docs/DECISIONS.md](docs/DECISIONS.md) (2026-07-31). Usa el AppImage o un
> paquete nativo.

La compilación desde el código fuente se documenta en
[CONTRIBUTING.md](CONTRIBUTING.md).
