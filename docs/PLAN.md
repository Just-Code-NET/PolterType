# Poltertype — План проєкту

> Жива дорожня карта. Оновлюється під час реалізації.
> Дата створення: 2026-05-02.

---

## 0. Журнал ключових рішень

| Дата | Рішення | Причина |
|---|---|---|
| 2026-05-02 | Початкова версія: Tauri 2 + Svelte 5. | Швидкий старт, готовий tray/autostart. |
| 2026-05-02 | **Перехід: pure Rust, без WebView. UI — `iced` + `tray-icon`.** | Користувач хоче «низькорівневіше і легше». Менший бінар, нема HTML-стека, цілісніша Rust-кодова база. |
| 2026-05-02 | **Закласти AI-pipeline як окрему підсистему.** | Користувач планує кастомні трюки з ML/LLM моделями. |
| 2026-05-02 | Bundle ID: `dev.opensource.poltertype`. | Зафіксовано користувачем. |
| 2026-05-02 | UI MVP: EN + UK. Архітектура — багатомовна, зокрема й «екзотичні». | Зафіксовано користувачем. |
| 2026-05-02 | Звуки: placeholder CC0 → пізніше власні. Інтерфейс «звукових тем» — заздалегідь гнучкий. | Зафіксовано користувачем. |
| 2026-05-02 | Default log level: `info`. | Зафіксовано користувачем. |
| 2026-05-02 | Реліз-канал v0.1: лише GitHub Releases. | Зафіксовано користувачем. |
| 2026-05-02 | **Wayland — у v0.1, як основний Linux-таргет.** X11 — fallback. | Сучасні дистрибутиви (GNOME/KDE) типово на Wayland; користувач хоче сфокусуватися на ньому. |
| 2026-05-02 | Phase 1 не вмикає `iced` вікно, лише tray + event loop. iced — у Phase 4. | Менше ризиків у каркасі; iced не потрібен, поки нема що показувати. |

---

## 1. Бачення продукту

**Poltertype** — крос-платформений (Windows / macOS / Linux) фоновий
застосунок, який автоматично перемикає розкладку клавіатури, коли
користувач почав вводити слово «не тією» розкладкою, і за можливості
виправляє вже введене слово (опційно зі звуковим підтвердженням).

Цільові якості:

- **Розумний** — детекція мови за лексичними/орфографічними ознаками,
  у перспективі — з підключенням ML/LLM. Помилкове перемикання — ворог №1.
- **Швидкий** — нативний код, нульова відчутна затримка вводу.
- **Легкий** — мінімальний бінар, нема WebView, нема Node-рантайму.
- **Непомітний** — живе у system tray, мінімум CPU/RAM, нуль телеметрії.
- **Гнучкий** — налаштовуваний whitelist/blacklist мов, гарячі клавіші,
  винятки, автокорекція on/off, опційні AI-плагіни.
- **Open source** — ліцензія MIT, репо на GitHub.

Натхнення: Punto Switcher, xneur, Caramba Switcher. Робимо сучасніше,
безпечніше, opt-in, без даркпатернів.

---

## 2. Технологічний стек

### 2.1 Чому pure Rust + `iced` (без WebView)

Розглянуті альтернативи:

| Варіант | Плюси | Мінуси |
|---|---|---|
| **Rust + `iced`** ✅ | Один runtime (Rust), бінар ~10–15 МБ, без HTML-стека. Декларативний UI у стилі Elm. MIT/Apache-2.0. | Менш «нативний» вигляд, ніж OS-widgets; необхідна власна композиція з tray-icon/global-hotkey. |
| Rust + `egui` (eframe) | Найшвидше прототипування, ще менший бінар. | Immediate-mode UI виглядає більше «developer-tool»; темізація обмежена. |
| Rust + Slint | Декларативний DSL, дуже сучасний look. | Ліцензія: GPLv3 / Royalty-Free / Commercial — несумісно з чистим MIT для нашого бінаря. |
| Rust + GTK4 (`gtk4-rs`) | Native-look на Linux. | На Win/macOS — важкі залежності, складна збірка. |
| Tauri 2 (Rust + WebView) | Зручний UI на Svelte/React. | Ваги WebView, два рантайми, веб-стек у repo (Node, Vite, Tailwind…). Користувач відмовився. |
| C++ (Qt) | Зрілість. | LGPL/комерційна морока, важкий runtime, ще одна мова поряд із Rust-ядром. |
| C++ (нативні OS API на кожній ОС) | Найвища швидкість і нативний look. | 3× кодова база, висока вартість підтримки. Не виправдано для tray-app. |
| Flutter Desktop / Electron / .NET MAUI | Великі рантайми, веб-стеки чи .NET. | Не вкладається в «низькорівнево і легко». |

**Висновок:** `Rust + iced` — найкращий компроміс «легке, сучасне,
один рантайм, MIT». Інтегруємо вручну з `tray-icon`, `global-hotkey`,
`auto-launch`. Якщо в процесі реалізації UX-вимоги до вікна налаштувань
виявляться занадто складними для `iced`, плануємо fallback на `egui`
(той самий рівень легкості, ще простіша інтеграція).

### 2.2 Ключові залежності (попередній перелік)

| Crate | Призначення |
|---|---|
| `iced` 0.13+ | UI вікна налаштувань |
| `tray-icon` | system tray (Win/Mac/Linux) |
| `global-hotkey` | глобальні гарячі клавіші |
| `auto-launch` | запуск при логіні (Win/Mac/Linux) |
| `single-instance` | заборона другого процесу |
| `tao` *(опційно)* | спільний event loop для tray + hotkeys + iced |
| `tokio` | async runtime, канали, таймери |
| `serde` / `serde_json` / `toml` | серіалізація налаштувань |
| `directories` | OS-specific шляхи (`~/.config`, `%APPDATA%`, ...) |
| `keyring` | secure storage для опційних API-ключів |
| `tracing` + `tracing-subscriber` + `tracing-appender` | логування |
| `parking_lot` | швидкі мьютекси |
| `crossbeam-channel` | lock-free черги між хук-потоком і engine |
| `lingua` *або* власний n-gram | базова детекція мови за словом |
| `unicode-normalization` | NFC/NFD для коректного порівняння |
| `rodio` | програвання WAV/OGG звуків |
| `notify-rust` | системні нотифікації (опційно) |
| **Win:** `windows` (офіційні Microsoft binding) | `WH_KEYBOARD_LL`, `LoadKeyboardLayoutW`, `SendInput`, `GetForegroundWindow`. |
| **macOS:** `core-graphics`, `core-foundation`, `objc2`, `objc2-app-kit` | `CGEventTap`, TIS API, `NSWorkspace`. |
| **Linux:** `x11rb`, `xkbcommon`, `evdev`, `ashpd` *(XDG portals)*, `libei` *(пізніше)* | XInput2, XKB, Wayland-сумісність. |
| **AI subsystem (опційно):** `ort` *(ONNX Runtime)* або `candle-core` | локальні моделі. |
| **AI subsystem (опційно):** `reqwest` + `eventsource-stream` | віддалені API (LLM). |

### 2.3 Без чого можна, але зручно

- `xtask`-pattern для допоміжних скриптів збірки замість `Makefile`.
- `cargo-deny` для перевірки ліцензій залежностей (важливо для MIT-проєкту).
- `cargo-dist` або власний release.yml для крос-збірки артефактів.

---

## 3. Архітектура

```
┌───────────────────────────────────────────────────────────┐
│  Main thread (event loop: tao)                            │
│  ┌────────────┐ ┌─────────────┐ ┌──────────────────────┐  │
│  │ TrayIcon   │ │GlobalHotkey │ │  iced Settings Window │  │
│  └────────────┘ └─────────────┘ │  (відкривається on-demand)│ │
│                                  └──────────────────────┘  │
└───────────────┬───────────────────────────────────────────┘
                │ (cmd channel)
┌───────────────▼───────────────────────────────────────────┐
│  CoreService (async, tokio)                                │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────────┐   │
│  │SettingsStore │ │ AudioPlayer  │ │ FocusTracker     │   │
│  └──────────────┘ └──────────────┘ └──────────────────┘   │
│                                                            │
│  ┌──────────────────────────────────────────────────────┐ │
│  │              SwitcherEngine  (state machine)          │ │
│  │   buffer ─► detector pipeline ─► decision policy ─►   │ │
│  │   ─► corrector                                        │ │
│  └──────┬─────────────────────────────────┬──────────────┘ │
│         │                                 │                │
│  ┌──────▼─────────┐               ┌──────▼─────────┐       │
│  │ InputListener  │               │ LayoutSwitcher │       │
│  │ (per-OS trait) │               │ (per-OS trait) │       │
│  └────────────────┘               └────────────────┘       │
└───────────────────────────────────────────────────────────┘

Detector pipeline (модульний, див. §3.4):
  HeuristicDetector  →  DictionaryDetector  →  [LocalMlDetector]
                                            →  [RemoteLlmDetector]
```

### 3.1 InputListener (глобальний хук клавіатури)

```rust
pub trait InputListener: Send + 'static {
    fn start(&mut self, events: crossbeam_channel::Sender<KeyEvent>) -> Result<()>;
    fn stop(&mut self);
}
```

Реалізації:

- **Windows** — `SetWindowsHookExW(WH_KEYBOARD_LL, ...)` у власному
  потоці з `GetMessageW` loop. Завжди повертаємо `CallNextHookEx`
  (нічого не блокуємо). Для виправлення слова — `SendInput`.
- **macOS** — `CGEventTapCreate(kCGSessionEventTap, ..., listenOnly)`,
  tap на CFRunLoop окремого потоку. Потрібен Accessibility permission —
  обов'язково перший-запуск-онбординг.
- **Linux Wayland (основний таргет)** — Wayland by-design не дає
  глобального keylogging API; реалістичні шляхи:
  1. **evdev** через `/dev/input/event*` (`evdev` crate). Потребує
     членства користувача в групі `input` + udev-правила. Ставимо
     під час встановлення скриптом `setup-linux.sh` (один `sudo`
     виклик, з чітким UI попередженням). Працює в усіх Wayland-
     композиторах (GNOME, KDE, Hyprland, Sway).
  2. **AT-SPI** через `atspi` crate як fallback, якщо користувач
     відмовився від групи `input` і має увімкнений accessibility-bus
     (GNOME — за замовчуванням, KDE — опційно). Менш надійно і
     повільніше, але без `sudo`.
  3. **`libei`** (`reis` crate) — для емуляції натискань (виправлення
     слова) через `org.freedesktop.portal.RemoteDesktop`/
     `InputCapture` portal у KDE 6.0+ / GNOME 46+. Це окремий шлях
     для send-keys, не listen-keys.

  Стратегія fallback: при старті визначаємо `XDG_SESSION_TYPE` і
  ефективні дозволи; якщо нічого не доступно — tray працює, але
  показує банер «keyboard hooks unavailable, see Setup».
- **Linux X11** — `XInput2 RawKeyPress` через `x11rb`. Не блокує.
  Залишаємо як fallback для X11-сесій.

### 3.2 LayoutSwitcher (зміна розкладки)

| ОС | API |
|---|---|
| Windows | `LoadKeyboardLayoutW` + `PostMessageW(HWND_BROADCAST, WM_INPUTLANGCHANGEREQUEST, ...)` або `ActivateKeyboardLayout`. |
| macOS | `TISCreateInputSourceList` → `TISSelectInputSource`. |
| Linux Wayland | Probe в порядку: Hyprland (`hyprctl`), KDE (`qdbus`), GSettings (`gsettings org.gnome.desktop.input-sources` — GNOME/Unity/Cinnamon/Budgie/Pantheon/MATE), IBus (`ibus engine`), Fcitx5 (`fcitx5-remote`). Кожен пробінг — реальний CLI/schema check, не просто env-guess. |
| Linux X11 | `XkbLockGroup` через `x11rb` (швидко) або fallback `setxkbmap -layout ...`. |

### 3.3 SwitcherEngine (логіка)

Стани:

1. `Idle` — нема активного слова.
2. `Buffering` — користувач набирає слово; ми збираємо `(scancode,
   vk, shift_state, current_layout, timestamp)`.
3. `Decide` — спрацював розділювач (Space, Enter, Tab, пунктуація).
   - Конвертуємо буфер у текст у кожній кандидатській розкладці
     (мапи накладок: EN↔UK, EN↔RU, EN↔DE, …).
   - Прогоняємо кожен варіант через **детектор-пайплайн** (§3.4).
   - Порівнюємо confidence; якщо альтернативна розкладка дає
     значно вищий score (поріг конфігуровний) — і відповідна мова
     є серед активних розкладок користувача — приймаємо рішення.
4. `Correct` — за згоди користувача (опція):
   - `len(buffer)` × `BackSpace` через `SendInput` / `CGEventPost` /
     `XTestFakeKeyEvent`.
   - Перемикаємо розкладку.
   - Шлемо коректний текст (через unicode-input або послідовність
     keydown/up згідно мапи).
   - Програємо звук.
5. **Hotkey Pause/Resume**, **Manual switch-last** (Ctrl+Shift+Backspace як
   у Punto): перемикає попереднє слово вручну.

Тригери, які скидають буфер:

- Зміна фокусу вікна.
- Хоткей паузи.
- Користувач сам перемкнув розкладку.
- Натискання редакторів буфера (Ctrl+Z, Ctrl+A, миша).
- Таймаут бездіяльності (наприклад, 2 с).

### 3.4 Detector pipeline (готовність до багатьох мов і AI)

**Це основний шов, у який вбудовується гнучкість.** Один трейт із
кількома реалізаціями, які виконуються послідовно або паралельно.

```rust
pub struct DetectionInput<'a> {
    pub raw_buffer: &'a [KeyStroke],
    pub current_layout: LayoutId,
    pub candidate_layouts: &'a [LayoutId],
    pub recent_context: &'a str,        // попередні N слів
    pub focused_app: Option<&'a AppId>, // для контексту
}

pub struct DetectionVerdict {
    pub best_layout: LayoutId,
    pub confidence: f32,           // 0.0–1.0
    pub reason: VerdictReason,     // для UI «чому перемкнули»
}

pub trait Detector: Send + Sync {
    fn name(&self) -> &'static str;
    fn detect(&self, input: &DetectionInput<'_>) -> Option<DetectionVerdict>;
}
```

Реалізації, які закладаємо:

| Detector | Призначення | Доступний |
|---|---|---|
| `HeuristicDetector` | швидкі правила: «у буфері немає літер цільової розкладки», апостроф/символ-маркери. | v0.1 |
| `DictionaryDetector` | n-gram + словник через `lingua-rs` або власні таблиці частотності. | v0.1 |
| `ContextDetector` | враховує попередні N слів (марковська модель). | v0.2 |
| `LocalMlDetector` | ONNX/Candle-модель (наприклад, fastText-style або TinyBERT). Працює офлайн. | v0.3 (опційно) |
| `RemoteLlmDetector` | API до OpenAI/Anthropic/локального Ollama. Лише за explicit-opt-in. | v0.3+ (плагін) |

Pipeline-policy (приклад):

```text
1. HeuristicDetector — якщо confidence > 0.95, accept.
2. DictionaryDetector — running detector for all candidate layouts;
   accept якщо delta(best, current) > threshold.
3. (опц.) LocalMlDetector — викликається лише коли (1)+(2) дали
   confidence < threshold і вмикання дозволене.
4. (опц.) RemoteLlmDetector — взагалі поза default; вмикається в
   налаштуваннях для «складних» полів (research, multi-lingual writing).
```

**Багатомовність:**

- Розкладка описується як `LayoutId` (BCP-47-подібний, напр. `uk-UA`,
  `en-US`, `de-DE`, `kk-Cyrl-KZ`, `hy-AM`).
- Мапа накладок описується даними у `src/layout/mappings/<id>.toml`
  (а не Rust-кодом), щоб додавання нової мови = додавання файлу.
- Детектори пишуться language-agnostic; знання про мови — у даних.
- Користувач у налаштуваннях бачить дві колонки: «доступні в системі»
  і «активні для Poltertype».

### 3.5 Зберігання налаштувань

`TOML` через `serde` (читабельне користувачем). Шлях:

- Win: `%APPDATA%\poltertype\config.toml`
- macOS: `~/Library/Application Support/poltertype/config.toml`
- Linux: `$XDG_CONFIG_HOME/poltertype/config.toml`

Структура (приклад):

```toml
schema_version = 1

[general]
autostart = true
sound_on_correct = true
show_notifications = false
ui_language = "system"     # або "en", "uk"
log_level = "info"

[languages]
active   = ["en-US", "uk-UA"]
ignored  = []

[engine]
min_word_length = 3
confidence_threshold = 0.85
ignore_in_password_fields = true
idle_timeout_ms = 2000

[exceptions]
disabled_apps    = ["Code.exe", "WindowsTerminal.exe"]
word_whitelist   = ["nginx", "kubectl", "github"]

[hotkeys]
pause_toggle        = "Ctrl+Shift+Space"
manual_switch_last  = "Ctrl+Shift+Backspace"

[sounds]
theme = "default"          # папка пресету
volume = 0.6

[ai]
enabled = false
# Список увімкнених AI-детекторів і їх конфіги:
# приклад див. §3.8
```

API-ключі **не зберігаються** в `config.toml` — лише посилання
на запис у системному кейчейні через `keyring`.

### 3.6 Tray UX

Іконка показує поточну розкладку (двосимвольний код, EN/UK/...) у вигляді
згенерованого PNG/ICO (за бажання — нативна композиція через
`tiny-skia`).

Меню:

- ✅ Active language: EN (US) ▸  *(submenu — швидке перемикання)*
- ⏸ Pause auto-switch
- ⚙ Settings…
- 📊 Today: 14 corrections
- 🪵 Open log
- ❌ Quit

### 3.7 Звуки (звукові теми)

- Звуки лежать у папках-темах: `<config>/sound-themes/<theme>/{correct,
  pause,switch,error}.ogg`. Bundled тема — `default/`.
- `AudioPlayer` шукає файл по логічному імені; якщо нема — silent (не
  падає).
- Гнучкість: користувач може створити свою тему, скопіювавши папку.
- v0.1 — placeholder CC0; пізніше — власні.

### 3.8 AI / ML підсистема (опційно вмикана)

**Дизайнерська ціль:** додати моделі ШІ так, щоб вони були окремою,
ізольованою, відключеною за замовчуванням підсистемою. Жодного
впливу на core-шлях, поки користувач явно не увімкнув.

Архітектура — через два шви:

#### A. Detector-плагіни (вже описано в §3.4)

`Box<dyn Detector>` додається в pipeline. Реалізації:

- `LocalOnnxDetector { model_path, runtime_threads }` — інференс через
  `ort`/`tract`/`candle`. Працює повністю офлайн.
- `RemoteLlmDetector { provider, model, api_key_ref, max_latency_ms }` —
  HTTP-запит з контекстом і кандидатами; провайдери:
  `openai`, `anthropic`, `ollama` (локальний, теж «remote» по
  схемі), `custom-openai-compatible`.

#### B. Word rewriter (нові кастомні «трюки»)

Окремий шов «після» детектора — перетворює навіть правильно набране
слово, якщо AI знає, що користувач мав на увазі інше. Для типу
використання: «авто-капіталізація», «розгортання акронімів», «заміна
slang → formal».

```rust
pub struct RewriteRequest<'a> {
    pub original: &'a str,
    pub layout: LayoutId,
    pub recent_context: &'a str,
}

pub enum RewriteVerdict {
    Keep,
    Replace { text: String, reason: String, requires_confirmation: bool },
}

pub trait WordRewriter: Send + Sync {
    fn name(&self) -> &'static str;
    fn rewrite(&self, req: &RewriteRequest<'_>) -> RewriteVerdict;
}
```

Rewriter завжди **підтверджує операцію** через дзеркальний flow до
тексто-корекції. За замовчуванням — disabled.

#### C. Конфіг прикладу

```toml
[ai]
enabled = true
default_pipeline = ["heuristic", "dictionary", "local-onnx"]
allow_remote = false   # окрема галочка: "розрішити мережеві виклики AI"

[[ai.detectors]]
type = "local-onnx"
id   = "fasttext-lid-176"
model_path = "models/lid.176.onnx"
threads = 1
weight = 1.0
min_text_length = 4

[[ai.detectors]]
type = "remote-llm"
id   = "anthropic-haiku"
provider = "anthropic"
model = "claude-haiku-4-5-20251001"
api_key_ref = "keyring:anthropic"   # ключ у системному кейчейні
max_latency_ms = 600
trigger_when_dictionary_below = 0.5
weight = 0.7

[[ai.rewriters]]
type = "remote-llm"
id   = "smart-capitalize"
provider = "openai"
model = "gpt-4o-mini"
api_key_ref = "keyring:openai"
prompt_template = "rewriters/smart_capitalize.tmpl"
require_confirmation = true
```

#### D. Гарантії приватності

- AI вимкнений за замовчуванням.
- Окремий toggle `allow_remote` — навіть якщо `enabled=true`, мережа
  заблокована, поки користувач явно не увімкне.
- На кожен remote-виклик у tray-tooltip є індикатор «AI:on/off,
  remote: yes/no» і лічильник «N AI calls today».
- API-ключі — через `keyring`, ніколи в plain-text.
- Cache LLM-відповідей за hash(input) — щоб не слати однакові
  слова повторно.

#### E. Динамічне завантаження плагінів (далекий план)

Спершу всі детектори/rewriter'и компілюються в бінар. Якщо знадобиться
сторонній маркетплейс — `wasmtime` для WASM-плагінів. Не в MVP.

### 3.9 FocusTracker (контекст застосунку)

- Win: `WinEventHookEx EVENT_SYSTEM_FOREGROUND`.
- macOS: `NSWorkspace.didActivateApplicationNotification`.
- Linux X11: `_NET_ACTIVE_WINDOW` property change.

Дає `AppId { exe_name, window_title }` для:

- per-app exceptions;
- скидання буфера при перемиканні фокусу;
- метаданих логів.

---

## 4. Структура репозиторію

```
poltertype/
├── .claude/
│   ├── settings.json
│   └── README.md
├── .github/
│   ├── workflows/
│   │   ├── ci.yml
│   │   └── release.yml
│   ├── ISSUE_TEMPLATE/
│   └── PULL_REQUEST_TEMPLATE.md
├── docs/
│   ├── PLAN.md                  # цей файл
│   ├── ARCHITECTURE.md          # глибше про модулі
│   ├── PERMISSIONS.md           # macOS Accessibility, Linux input group
│   ├── AI.md                    # як підключити модель/API
│   ├── ADDING_A_LANGUAGE.md
│   └── CONTRIBUTING.md
├── crates/
│   ├── poltertype-app/                  # бінар (main, tray, window, IPC)
│   │   ├── Cargo.toml
│   │   └── src/{main.rs, tray.rs, ui/, ipc.rs}
│   ├── poltertype-core/                 # SwitcherEngine, налаштування, event-loop
│   │   ├── Cargo.toml
│   │   └── src/{engine/, settings/, focus.rs, audio.rs, autostart.rs}
│   ├── poltertype-input/                # InputListener trait + per-OS
│   │   └── src/{lib.rs, windows.rs, macos.rs, linux.rs}
│   ├── poltertype-layout/               # LayoutSwitcher trait + per-OS
│   │   └── src/{lib.rs, windows.rs, macos.rs, linux.rs, mappings/}
│   ├── poltertype-detect/               # Detector pipeline (heuristic/dict/...)
│   │   └── src/{lib.rs, heuristic.rs, dictionary.rs, context.rs}
│   ├── poltertype-ai/                   # ОПЦІЙНО (feature `ai`)
│   │   └── src/{lib.rs, local_onnx.rs, remote_llm.rs, rewriters/}
│   └── poltertype-types/                # спільні типи (LayoutId, KeyEvent, ...)
├── assets/
│   ├── icons/
│   ├── tray/                    # шаблони для генерованих іконок
│   └── sound-themes/
│       └── default/
│           ├── correct.ogg
│           ├── pause.ogg
│           └── switch.ogg
├── data/
│   └── layout-mappings/         # TOML-файли накладок
│       ├── en_us.toml
│       ├── uk_ua.toml
│       └── ...
├── tests/                       # інтеграційні тести (без OS-хуків)
│   ├── engine_decision.rs
│   └── layout_mapping.rs
├── xtask/                       # допоміжні скрипти збірки
│   └── src/main.rs
├── Cargo.toml                   # workspace
├── Cargo.lock
├── rust-toolchain.toml          # фіксуємо stable + components
├── deny.toml                    # cargo-deny: license/duplicate checks
├── rustfmt.toml
├── clippy.toml
├── .editorconfig
├── .gitignore
├── .gitattributes
├── CLAUDE.md
├── LICENSE                      # MIT
└── README.md
```

Workspace з декількох крейтів дає:

- Чисту ізоляцію OS-коду під `#[cfg(...)]` лише в `poltertype-input` /
  `poltertype-layout`.
- AI-крейт за `feature = "ai"` — за замовчуванням не компілюється.
- Можливість винести `poltertype-detect` як окрему бібліотеку, якщо знадобиться
  стороннє використання.

---

## 5. Інтеграція з Claude Code

### 5.1 `CLAUDE.md` (root)

Завжди в контексті:

- Архітектурні правила (де живе платформенний код, як додавати нову мову).
- Команди розробки (`cargo run -p poltertype-app`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo fmt --all`).
- Стиль коду (rustfmt, clippy strict).
- Безпекові обмеження (не логувати тексту користувача в release).
- Релізна процедура.

### 5.2 `.claude/settings.json`

- Дозволи на безпечні тули (`Read`, `Grep`, `Glob`, `Edit`, `Write`,
  `Bash(cargo *)`, `Bash(rustup *)`, `Bash(git status / diff / log / ...)`).
- Заборонити випадкові пуш/форс-операції.

### 5.3 Можливі subagents (пізніше)

- `platform-windows-expert` / `platform-macos-expert` /
  `platform-linux-expert`.
- `layout-mapping` — допомога з додаванням нових розкладок.
- `ai-integrations` — для роботи з AI-плагінами.

---

## 6. Безпека і приватність

- Жодної мережі за замовчуванням. AI вимкнений; remote-AI потребує
  двох галочок (`enabled` + `allow_remote`).
- Не зберігаємо текст. Тільки коротко-живий буфер слова в RAM,
  очищується після рішення/таймауту.
- API-ключі — через `keyring` (Win Credential Manager / macOS
  Keychain / GNOME Secret Service / KWallet).
- Парольні поля:
  - Win: пропускаємо, якщо у фокусі поле з `ES_PASSWORD`.
  - macOS: `AXSecureTextField`.
  - Linux: евристика + опція «вимкнути в …».
- Лог: рівень за замовчуванням `info`. У release не логується вміст
  буфера, лише metadata (довжина, мова-від, мова-до).
- Підпис релізних бінарів — окрема фаза.

---

## 7. Тестування

Рівні:

1. **Unit (Rust):**
   - `poltertype-detect::heuristic` — таблиці тестових слів і очікуваних
     рішень.
   - `poltertype-layout::mappings` — повна симетрія мап (EN→UK→EN = identity).
2. **Integration:**
   - Інжектуємо синтетичні `KeyEvent` у `SwitcherEngine`, перевіряємо
     рішення без OS-хуків.
   - Property-based тести (`proptest`) для random-input fuzzing engine.
3. **E2E (manual matrix):**
   - Win 11, macOS 14+ (Intel+ARM), Ubuntu 24.04 (X11 + Wayland),
     Fedora 40 (Wayland).
4. **CI:**
   - cargo fmt / clippy / test на матриці {windows-latest,
     macos-latest, ubuntu-latest}.
   - cargo-deny на ліцензії.

---

## 8. Ризики та невідомі

| Ризик | Вплив | План мітигації |
|---|---|---|
| Wayland не дає глобального keylogging API. | Це блокер для основного use-case на Wayland. | Шлях через evdev + group `input` + udev rule (вмикається `setup-linux.sh`). AT-SPI fallback. Tray onboarding пояснює, що відбувається й чому. |
| `setup-linux.sh` лякає `sudo`-prompt'ом. | Drop-off на онбордингу. | Чесний пояснювальний банер у tray + посилання на код скрипта. Альтернативно — гайд як зробити вручну. |
| macOS Accessibility prompt лякає. | Drop-off на онбордингу. | Перший запуск: чистий гайд із GIF. |
| Помилкові спрацювання детектора. | Дратує найбільше. | Високий threshold + Undo-хоткей + статистика «скасованих» спрацювань для тюнінгу. |
| Антивірус/SmartScreen на Windows. | Користувач не запустить білд без підпису. | Перший реліз — попередження в README. |
| Програми, які самі ловлять глобальні хуки (ігри). | Конфлікт. | Per-app disable-list. |
| Перформанс на старих машинах. | Заїкання вводу. | Hook-callback тільки enqueue; обробка — окремий потік. |
| AI-залежності (ONNX runtime) роздувають бінар. | Великі MB. | `feature = "ai"`; AI-збірка — окремий артефакт `poltertype-ai`. |
| Remote LLM API: latency 200–800 мс. | Не вкладається в інлайн-корекцію. | Викликати тільки в "background-rewrite" режимі (післяфактум) із підтвердженням. |
| Headless Linux audio. | Падіння при старті. | `rodio` ліниво ініціалізується, fallback no-op. |

---

## 9. Зафіксовані рішення (попередньо «відкриті питання»)

1. **UI-фреймворк:** `iced` (pure Rust). Fallback — `egui`.
2. **Bundle ID:** `dev.opensource.poltertype`.
3. **Мови UI v0.1:** EN + UK; архітектура — багатомовна (i18n через
   `fluent-rs` або `rust-i18n`, файли `.ftl` у `assets/i18n/`).
4. **Звуки v0.1:** CC0 placeholder; формат тем — папки.
5. **Лог за замовчуванням:** `info`.
6. **Реліз-канал v0.1:** GitHub Releases only.

---

## 10. Дорожня карта

### Фаза 0 — Каркас (зараз, без коду логіки)

- [x] Створити проєкт, `git init`.
- [x] PLAN.md, README, LICENSE (MIT), .gitignore, .gitattributes,
      .editorconfig, CLAUDE.md, `.claude/`.
- [ ] (опційно зараз) ADR-шаблон, CONTRIBUTING.md.

### Фаза 1 — Bootstrap Rust-каркасу

- [ ] Cargo workspace з 7 крейтами (порожні `lib.rs`).
- [ ] `poltertype-app`: `tao` event loop + `tray-icon` (Settings/Quit меню,
      генерована placeholder-іконка). `iced` вікно — у Phase 4.
- [ ] `single-instance`, `tracing` ініціалізація.
- [ ] CI: `cargo fmt/clippy/check` на трьох ОС.
- [ ] `cargo-deny` базова конфігурація.

### Фаза 2 — Платформенні адаптери (skeleton)

- [ ] `poltertype-input`: trait + Windows-реалізація LL hook (лише log).
- [ ] `poltertype-layout`: trait + Windows-реалізація.
- [ ] Stub'и для macOS / Linux (компілюються, повертають `Unsupported`).
- [ ] `docs/PERMISSIONS.md` із описом для macOS/Linux.

### Фаза 3 — SwitcherEngine MVP

- [ ] `poltertype-types`: спільні типи (LayoutId, KeyEvent, ...).
- [ ] `poltertype-detect`: `HeuristicDetector` + `DictionaryDetector` (lingua).
- [ ] `poltertype-core`: WordBuffer, DecisionPolicy, Corrector, AudioPlayer.
- [ ] EN↔UK мапа в `data/layout-mappings/`.
- [ ] Pause/Undo хоткей.
- [ ] Налаштування: збереження/завантаження `config.toml`.

### Фаза 4 — Settings UX (без повноцінного GUI у v0.1)

Див. `docs/DECISIONS.md` запис `2026-05-02 — Phase 4: deferred full
GUI`. Замість `iced`-сторінок Phase 4 робить:

- [ ] Tray menu: "Open Settings" → відкриває `config.toml` в
      редакторі за замовчуванням (cross-platform `opener`).
- [ ] Tray menu: "Open Logs" → відкриває папку з логами.
- [ ] Tray menu: "Reload Settings" → перечитує config + повідомляє
      engine.
- [ ] File-backed logs через `tracing-appender` (daily rotation).
- [ ] Engine: фільтрація candidate layouts за `[languages].active` /
      `[languages].ignored`.
- [ ] Повноцінний GUI (`iced` чи `egui`) — Phase 8 / v0.2, коли
      зрозуміла поведінка event-loop'ів на macOS і Linux.

### Фаза 5 — macOS повністю

- [ ] `CGEventTap` + Accessibility onboarding.
- [ ] `TISSelectInputSource`.
- [ ] `NSWorkspace` focus tracking.

### Фаза 6 — Linux (Wayland-first)

- [ ] **Wayland evdev listener** через `evdev` crate; `setup-linux.sh`
      для додавання користувача в групу `input` + udev-правило.
- [ ] Wayland AT-SPI fallback listener через `atspi`.
- [ ] Layout-switcher через D-Bus (GNOME → KDE → IBus → Fcitx у такому
      порядку), кожна реалізація — окремий бекенд за `Trait`.
- [ ] Send-keys (виправлення слова): через `uinput` (paired з evdev)
      і `libei` (`reis`) як портал-варіант.
- [ ] X11 fallback: XInput2 listener + XkbLockGroup switcher.
- [ ] Onboarding-банер: пояснення, чому потрібен `sudo`, посилання на
      скрипт, кнопка «Run setup».

### Фаза 7 — AI каркас (опційно)

- [ ] `poltertype-ai` крейт за `feature = "ai"`.
- [ ] `Detector` + `WordRewriter` traits інтегровано в pipeline.
- [ ] Один еталонний `LocalOnnxDetector` із `lid.176`.
- [ ] Один еталонний `RemoteLlmDetector` (Anthropic API) з
      `keyring`-сховищем ключа та UI-онбордингом.
- [ ] `docs/AI.md`.

### Фаза 8 — Polish, beta-реліз 0.1

- [ ] Іконки, переклад UI, скріншоти в README.
- [ ] GitHub Action — артефакти на тег.
- [ ] Reddit/HN ann (опційно).

### Фаза 9 (пізніше)

- Інсталятори, підпис, magazines (winget, brew, AUR).
- Маркетплейс плагінів (WASM).

---

## 11. Метрики готовності v0.1 (definition-of-done)

- Працює на Windows 11 повний цикл: «`руддщ` → `hello`, звук».
- Працює на macOS 14+ (Intel+ARM): теж.
- Працює на Wayland Ubuntu 24.04 / Fedora 40 (GNOME або KDE) повний
  цикл після `setup-linux.sh`. X11 — як fallback працює без скрипта.
- Tray показує мову, меню працює.
- UI вікно: General + Languages + Hotkeys + Exceptions сторінки
  робочі, налаштування зберігаються.
- AI підсистема **присутня в коді** як вимкнений `feature = "ai"`,
  з прикладом конфігу і документацією, навіть якщо не оснащена
  готовою моделлю.
- README має інструкцію збірки і скріншоти.
- CI зелений на трьох ОС.
