# PolterType — План проєкту

> Жива дорожня карта. Оновлюється під час реалізації.
> Дата створення: 2026-05-02. Актуалізовано: 2026-07-11 (v0.2.0).

> **Як читати цей документ.** Це **план**, а не опис реалізації. Там,
> де код розійшовся з задумом, істина — код, а не цей файл. Найсвіжіші
> зведення:
>
> * **Що вже вийшло** — `CHANGELOG.md` (0.1.0 «First stable», 0.1.1,
>   0.2.0) і §10 нижче, де відмічено кожен пункт.
> * **Чому саме так** — `DECISIONS.md`; кілька рішень нижче вже
>   переглянуті (найпомітніше — «повноцінний GUI відкладено», хоч він
>   вийшов ще у 0.1.0-beta).
> * **Чого немає, попри те що описано нижче** — `../CLAUDE.md`,
>   розділ «Known gaps»: AI-підсистема не під'єднана до движка,
>   `FocusTracker` реалізований лише на Windows, AT-SPI / `libei` /
>   онбординг-вікна / tray-банерів не існує.
>
> Секції 2–4 подекуди описують початковий задум (залежності, яких так
> і не взяли; меню трея, яке склалося інакше). Звіряйтесь із кодом.

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
| 2026-05-07 | **Перегляд:** повноцінне `iced`-вікно виходить уже в 0.1, а не «Phase 8 / v0.2». Сім панелей, окремий процес `--settings`. | Поведінка event-loop'ів прояснилася раніше, ніж очікували; окремий процес знімає конфлікт з main-thread на macOS. |
| 2026-05-07 | **Перегляд:** дані (розкладки, словники) винесені з бінаря у `<data_dir>/`, замість `include_str!`/`include_bytes!`. | Ліниве завантаження за активними розкладками; можливість user-оверлеїв і плагін-паків без перезбірки. |
| 2026-05-21 | **v0.1.0 — вихід із бети** («First stable»). | Wayland-шлях (Hyprland + keyd) стабільно працює на щоденній машині мейнтейнера. |
| 2026-07-11 | **Перегляд: X11 — не «fallback», а повноцінний шлях.** Єдиний тип Linux-сесії, що працює **без жодних дозволів** (ні `input`-групи, ні `sudo`, ні `setup-linux.sh`). | XInput2 + XTest доступні будь-якому клієнту, що відкрив дисплей. Іронічно — найнижчий поріг входу з усього Linux. |
| 2026-07-11 | **Перейменування `kb-switcher` → PolterType** — бінар, крейти, app id, конфіг-каталог, env-var. Старий конфіг переймається автоматично при першому запуску. | Робоча назва вичерпала себе; міграція, щоб не втратити налаштування наявних користувачів. |
| 2026-07-11 | **Correction pipeline v2**: спершу перемкнути розкладку, потім видаляти (а не навпаки). | Детально в `DECISIONS.md` (запис від 2026-07-11): усуває гонку з echo від власного емітера. |

---

## 1. Бачення продукту

**PolterType** — крос-платформений (Windows / macOS / Linux) фоновий
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

Реалізації:

> ⚠️ Сигнатура трейта вище — з початкового задуму. **У коді вона
> інша:** `fn judge(&self, ctx: &DetectionContext<'_>) -> Verdict`, де
> `Verdict` тризначний (`NoOpinion` / `Keep { reason }` / `Switch`).
> Саме `Keep` дозволяє словнику сказати «це справжнє слово, не
> чіпайте» — головний запобіжник від хибних спрацювань.

| Detector | Призначення | Стан |
|---|---|---|
| `WordPlausibilityDetector` (планувався як `HeuristicDetector`) | швидкі правила: чи виглядає слово правдоподібним для поточної розкладки (літери, частка голосних, нагромадження приголосних). | ✅ у 0.1 |
| `DictionaryDetector` | FST-словник по Hunspell-розгорнутих списках. `lingua-rs` і n-grams **не використали** — від них відмовились на користь FST. | ✅ у 0.1 |
| `ContextDetector` | враховує попередні N слів (марковська модель). | ❌ немає (планувався на v0.2 — не зроблений) |
| `LocalOnnxDetector` | ONNX-модель, офлайн. | 🚧 стаб у `poltertype-ai`, до движка не під'єднаний |
| `RemoteLlmDetector` | API до OpenAI/Anthropic/локального Ollama. Лише за explicit-opt-in. | 🚧 стаб; мережевих викликів не робить жодна збірка |

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
  і «активні для PolterType».

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

Меню — **як склалося у коді** (початковий ескіз із submenu швидкого
перемикання та лічильником «Today: N corrections» реалізований **не
був**):

- ⚙ Settings…
- 📝 Edit config.toml…
- 🪵 Open Logs Folder…
- 📖 Open User Wordlists Folder…
- ⌨ Open User Layouts Folder…
- 🔄 Reload Settings
- ⏸ Pause auto-switch
- ℹ About …
- ❌ Quit

### 3.7 Звуки (звукові теми)

- **Типово звуки синтезуються** — `AudioPlayer` генерує тон на льоту
  (різна висота на подію). Так бінар лишається малим і немає
  per-platform клопоту з декодерами. Каталогу `assets/` у репозиторії
  **немає**, bundled-теми `default/` теж.
- Користувацька тема: `<config-dir>/sound-themes/<theme>/<event>.ogg`.
  Події — `correct`, `pause`, `resume` (не `{correct,pause,switch,
  error}`, як планувалося).
- Якщо файлу теми нема — тихо відкочуємось на синтезований тон, не
  падаємо.

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

> Станом на v0.2.0 гарантія сильніша за задуману: **мережевого коду
> просто немає**. Підсистема до движка не під'єднана, тож усе нижче —
> вимоги до майбутньої реалізації, а не опис поточної поведінки.
> Індикатора в tray-tooltip і лічильника викликів **не існує** —
> tooltip показує лише назву, розкладку і «(paused)».

- AI вимкнений за замовчуванням.
- Окремий toggle `allow_remote` — навіть якщо `enabled=true`, мережа
  має лишатись заблокованою, поки користувач явно не увімкне.
  (Сьогодні прапорець парситься, але його не читає жоден код.)
- На кожен remote-виклик у tray-tooltip має бути індикатор «AI:on/off,
  remote: yes/no» і лічильник «N AI calls today» — **не реалізовано**.
- API-ключі — через `keyring`, ніколи в plain-text. (Хелпер написаний;
  викликати його поки нема кому.)
- Cache LLM-відповідей за hash(input) — щоб не слати однакові
  слова повторно.

#### E. Динамічне завантаження плагінів (далекий план)

Спершу всі детектори/rewriter'и компілюються в бінар. Якщо знадобиться
сторонній маркетплейс — `wasmtime` для WASM-плагінів. Не в MVP.

### 3.9 FocusTracker (контекст застосунку)

- Win: `WinEventHookEx EVENT_SYSTEM_FOREGROUND` — **✅ реалізовано**.
- macOS: `NSWorkspace.didActivateApplicationNotification` — **❌ ні**.
- Linux X11: `_NET_ACTIVE_WINDOW` property change — **❌ ні**.

> **Це найтихіша діра в продукті.** `create_focus_tracker()` повертає
> `NoopFocusTracker` на всьому, що не Windows, а його `focused_exe()`
> завжди віддає `None`. Отже все, що зав'язане на активний застосунок,
> на macOS і Linux **мовчки не працює**: `[exceptions].disabled_apps`,
> per-app профілі словників, та `apps = [...]` у smart-командах.
> Помилки не буде — просто нічого не станеться. Не обіцяйте цих
> можливостей для не-Windows (зокрема на лендінгу), поки трекери не
> з'являться.

Дає `AppId { exe_name, window_title }` для:

- per-app exceptions;
- скидання буфера при перемиканні фокусу;
- метаданих логів.

---

## 4. Структура репозиторію

Фактична структура на v0.2.0 (початковий ескіз розходився з нею в
кількох місцях: `assets/` і кореневого `tests/` не існує, модулі
розбиті по каталогах «одна сутність — один файл», а `CONTRIBUTING.md`
лежить у корені, не в `docs/`):

```
poltertype/                      # (конфіг Claude — не тут, а в корені
│                                #  воркспейсу: ../.claude/)
├── .cargo/config.toml           # аліас `cargo xtask`
├── .github/workflows/{ci.yml,release.yml}
├── .githooks/                   # pre-commit / pre-push (ставляться xtask'ом)
├── docs/
│   ├── PLAN.md                  # цей файл
│   ├── DECISIONS.md             # журнал архітектурних рішень
│   ├── DATA_LAYOUT.md           # дерево даних на диску + плагіни
│   ├── PERMISSIONS.md           # macOS Accessibility, Linux evdev/X11
│   ├── AI.md                    # стан і задум AI-підсистеми
│   ├── ADDING_A_LANGUAGE.md
│   └── RELEASING.md
├── crates/
│   ├── poltertype-app/          # бінар: tray, Settings-UI (окремий процес)
│   │   └── src/{main.rs, tray.rs, detectors.rs, settings_ui/, settings_proc.rs, icon_render/}
│   ├── poltertype-core/         # engine, settings, layouts, commands, audio
│   │   └── src/{engine/, settings/, layouts/, commands/, wordlist_profiles/, audio/, data_dir/}
│   │       └── build.rs         # готує target/dist/data з data/
│   ├── poltertype-input/        # InputListener + KeyEmitter + FocusTracker
│   │   └── src/{windows/, macos/, linux/{wayland,x11}/, focus/}
│   ├── poltertype-layout/       # LayoutSwitcher + per-OS бекенди
│   │   └── src/{windows/, macos/, linux/{hyprland,kde,gsettings,ibus,fcitx,x11}/}
│   ├── poltertype-detect/       # Detector pipeline
│   │   └── src/{traits.rs, plausibility.rs, dictionary.rs, enums.rs}
│   ├── poltertype-ai/           # ОПЦІЙНО (feature `ai`); стаби, не під'єднано
│   │   └── src/{local.rs, remote/, rewriters.rs, keys.rs}
│   └── poltertype-types/        # спільні типи (LayoutId, KeyEvent, ...)
├── data/                        # джерело правди, консумиться build.rs
│   ├── layout-mappings/         # TOML-накладки (en_us.toml, uk_ua.toml, ...)
│   └── wordlists/               # <stem>.txt.gz + -extras/-stop/-weak
├── installers/{wix,windows,macos,linux}/
├── scripts/setup-linux.sh
├── xtask/                       # wordlists fetch, hooks install, icon, version
├── Cargo.toml                   # workspace
├── CHANGELOG.md
├── CONTRIBUTING.md
├── CLAUDE.md
├── LICENSE                      # MIT
└── README.md
```

Інтеграційних тестів у кореневому `tests/` немає — юніт-тести лежать
у сусідніх `tests.rs` всередині кожного модуля (див. CONTRIBUTING.md,
розділ про організацію файлів).

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

> **Статус на v0.2.0.** Фази 0–8 у своїй основній частині
> завершені й вийшли в релізах 0.1.0 → 0.2.0; нижче відмічено, що
> саме лишилося відкритим. Пункти, які **не** зроблені, свідомо
> лишені як `[ ]` — це і є актуальний список робіт. Формулювання
> самих пунктів подекуди відстало від коду (напр. `HeuristicDetector`
> у Фазі 3 насправді зветься `WordPlausibilityDetector`); тут
> виправлено.

### Фаза 0 — Каркас ✅

- [x] Створити проєкт, `git init`.
- [x] PLAN.md, README, LICENSE (MIT), .gitignore, .gitattributes,
      .editorconfig, CLAUDE.md, `.claude/`.
- [x] CONTRIBUTING.md (у корені репозиторію, не в `docs/`).

### Фаза 1 — Bootstrap Rust-каркасу ✅

- [x] Cargo workspace з 7 крейтами.
- [x] `poltertype-app`: `tao` event loop + `tray-icon`, генерована
      placeholder-іконка.
- [x] `single-instance`, `tracing` ініціалізація.
- [x] CI: `cargo fmt/clippy/check` на трьох ОС.
- [x] `cargo-deny` базова конфігурація.

### Фаза 2 — Платформенні адаптери ✅

- [x] `poltertype-input`: trait + Windows LL hook.
- [x] `poltertype-layout`: trait + Windows-реалізація.
- [x] macOS / Linux більше **не** stub'и — див. Фази 5 і 6.
- [x] `docs/PERMISSIONS.md`.

### Фаза 3 — SwitcherEngine MVP ✅

- [x] `poltertype-types`: спільні типи (LayoutId, KeyEvent, ...).
- [x] `poltertype-detect`: `WordPlausibilityDetector` +
      `DictionaryDetector`. Словник — FST по Hunspell-розгорнутих
      списках (не `lingua`, від якої відмовились).
- [x] `poltertype-core`: WordBuffer, DecisionPolicy, Corrector,
      AudioPlayer.
- [x] EN↔UK мапа в `data/layout-mappings/` (сьогодні бандлиться
      шість: EN·UK·RU·DE·ES·FR).
- [x] Pause / switch-last хоткеї.
- [x] Налаштування: збереження/завантаження `config.toml`.

### Фаза 4 — Settings UX ✅

Початковий план (див. `docs/DECISIONS.md`, запис
`2026-05-02 — Phase 4: deferred full GUI`) відкладав повноцінне
вікно. **Це рішення згодом переглянули**: `iced`-GUI вийшов ще у
0.1.0-beta, і сьогодні має сім панелей (Languages, Hotkeys, Commands,
Wordlists, General, Exceptions, About). Запускається як окремий
процес `poltertype --settings`.

- [x] Tray menu: "Edit config.toml…" через `opener`.
- [x] Tray menu: "Open Logs Folder…".
- [x] Tray menu: "Reload Settings".
- [x] File-backed logs через `tracing-appender`.
- [x] Engine: фільтрація candidate layouts за `[languages]`.
- [x] Повноцінний GUI (`iced`) — вийшов раніше, ніж планувалось.

### Фаза 5 — macOS

- [x] `CGEventTap` (listener) — написано за документацією Apple,
      перевірено лише на CI.
- [x] `TISSelectInputSource` (перемикання розкладки).
- [ ] **Accessibility onboarding** — вікна першого запуску немає.
- [ ] **`NSWorkspace` focus tracking** — не реалізовано, тож
      `FocusTracker` на macOS це no-op (див. Фазу 6 і §3.9).
- [ ] **Runtime-перевірка на живому залізі.** Найбільша відкрита
      позиція по macOS: жоден із бекендів не проганявся на реальній
      машині.

### Фаза 6 — Linux

- [x] **Wayland evdev listener** через `evdev`; `setup-linux.sh`
      додає в групу `input` + udev-правила (`/dev/input/event*` та
      `/dev/uinput`).
- [x] Layout-switcher: Hyprland → KDE → GSettings (GNOME-родина) →
      IBus → Fcitx5 → X11 XKB, кожен окремим бекендом за `Trait`.
- [x] Send-keys через `uinput` (у парі з evdev).
- [x] X11: XInput2 listener + XTest emitter + XKB-світчер
      (`XkbLatchLockState`). Потребує нуль дозволів (ні
      `input`-групи, ні `sudo`). XKB-світчер пробується **останнім**
      — там, де сесією керує DE, його бекенд тримає індикатор
      розкладки в синхроні, а замикання групи під ним лишило б
      індикатор брехати.
- [ ] **Wayland AT-SPI fallback listener** через `atspi` — не
      реалізовано (залежності немає в дереві).
- [ ] **`libei` (`reis`) як портал-варіант send-keys** — не
      реалізовано; `uinput` наразі єдиний шлях.
- [ ] **Onboarding-банер** із кнопкою «Run setup» — не реалізовано.
- [ ] **`FocusTracker` для Linux** (`_NET_ACTIVE_WINDOW` / Wayland) —
      не реалізовано, див. §3.9.

### Фаза 7 — AI каркас

Каркас є, **але до движка не під'єднаний** — жоден рядок
`poltertype-app` / `poltertype-core` не імпортує `poltertype-ai`.
Детальніше в `docs/AI.md`.

- [x] `poltertype-ai` крейт за `feature = "ai"` (вимкнений типово).
- [x] `Detector` + `WordRewriter` traits оголошені в
      `poltertype-detect`.
- [x] `docs/AI.md`.
- [ ] **Traits інтегровано в pipeline** — ні. Список детекторів
      захардкоджений у `poltertype-app::main`; схеми
      `[[ai.detectors]]` в налаштуваннях не існує, ключі
      `[ai].enabled` / `allow_remote` не читає жоден код.
- [ ] Еталонний `LocalOnnxDetector` із `lid.176` — стаб.
- [ ] Еталонний `RemoteLlmDetector` (Anthropic API) — стаб; мережевих
      викликів не робить жодна збірка.

### Фаза 8 — Polish, реліз ✅ (частково)

- [x] Іконки (рендеряться з `xtask assets icon-png`).
- [x] GitHub Action — артефакти на тег (`release.yml`).
- [x] **Інсталятори**: MSI (WiX), universal DMG, AppImage. Вийшли
      разом з 0.1 — раніше, ніж планувала Фаза 9.
- [ ] Переклад UI (i18n) — інтерфейс лише англійський.
- [ ] Скріншоти в README.

### Фаза 9 (пізніше)

- **Підпис** інсталяторів (Apple Developer ID, Windows EV/OV) —
  сьогодні всі артефакти **непідписані**.
- Магазини: winget, brew, AUR, Microsoft Store.
- Маркетплейс плагінів: лоадер живий (data-only паки), відкритий
  лишається UX встановлення / оновлення / підпису пака. WASM-плагіни
  — окремо.

---

## 11. Метрики готовності v0.1 (definition-of-done)

> **v0.1.0 вийшов** (реліз «First stable», див. CHANGELOG). Нижче —
> що з цього списку справдилось, а що ні.

- [x] Windows: повний цикл «`руддщ` → `hello`, звук».
- [x] Linux: повний цикл на Wayland (Hyprland + keyd — щоденна
      машина мейнтейнера) після `setup-linux.sh`; на X11 — **без
      жодного скрипта** (це вже не «fallback», а повноцінний шлях).
- [ ] macOS (Intel+ARM): збирається і проходить CI, але на живому
      залізі цикл ніхто не проганяв. Єдиний непідтверджений пункт.
- [x] Tray показує мову, меню працює.
- [x] UI вікно: сім панелей (більше, ніж планувалось), налаштування
      зберігаються.
- [x] AI підсистема присутня в коді як вимкнений `feature = "ai"` з
      документацією — але, всупереч початковому формулюванню, вона
      **не під'єднана до pipeline** (див. Фазу 7).
- [ ] Скріншоти в README — немає (інструкція збірки є).
- [x] CI зелений на трьох ОС.
