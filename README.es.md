# PolterType

Cambiador automático de distribución de teclado multiplataforma. Vive en la
bandeja del sistema, detecta cuando empiezas a escribir en la distribución
incorrecta, cambia de distribución y vuelve a escribir la última palabra, como
un poltergeist amable que habita tu teclado.

Para la documentación completa, las notas de desarrollo y las advertencias
detalladas, consulta el [README en inglés](README.md).

## Instalación

Los binarios se publican en la [página de Releases](../../releases). Cada
release incluye cuatro instaladores:

| Sistema | Archivo | Cómo instalar |
| --- | --- | --- |
| Windows 10 / 11 | `poltertype-<ver>-x86_64-pc-windows-msvc.msi` | Doble clic. Instalación por usuario, sin derechos de administrador ni UAC. SmartScreen puede mostrar "Windows protegió tu PC" → **Más información** → **Ejecutar de todas formas**. |
| macOS 11+ (Intel y Apple Silicon) | `poltertype-<ver>-universal-apple-darwin.dmg` | Abre el DMG y arrastra `poltertype.app` a `/Applications`. En el primer lanzamiento, haz clic derecho sobre la app → **Abrir** (o ejecuta `xattr -dr com.apple.quarantine /Applications/poltertype.app`). Luego concede **Accesibilidad** y **Monitoreo de entrada** cuando macOS lo solicite. |
| Linux (x86_64) | `poltertype-<ver>-x86_64.AppImage` | `chmod +x` y ejecuta. Instalación por usuario, sin instalación del sistema. Consulta [docs/PERMISSIONS.md](docs/PERMISSIONS.md) para el acceso `evdev` en Wayland. |
| Linux (aarch64) | `poltertype-<ver>-aarch64.AppImage` | Igual que arriba, para ARM64: Raspberry Pi 5, Asahi, laptops y servidores ARM. |

> Los instaladores todavía **no están firmados**, por eso Gatekeeper o
> SmartScreen advierten en el primer lanzamiento. La firma de código llegará en
> una fase posterior.

> **No hay Flatpak y no lo habrá.** PolterType escribe en `/dev/uinput`, lo que
> ningún permiso de Flatpak concede salvo `--device=all` (todo el árbol de
> dispositivos), y no existe un portal para ello. El cambio de distribución
> también necesita binarios del sistema (`hyprctl`, `gsettings`, `qdbus`,
> `ibus`) que una sandbox no tiene. El razonamiento, las fuentes y las
> condiciones bajo las cuales lo reconsideraríamos están en
> [docs/DECISIONS.md](docs/DECISIONS.md) (2026-07-31). Usa el AppImage o un
> paquete nativo.

La compilación desde el código fuente se documenta en
[CONTRIBUTING.md](CONTRIBUTING.md).
