//! Schema versioning for `config.toml`, and the frozen historical
//! defaults a migration has to recognise.

pub(crate) const SCHEMA_VERSION: u32 = 1;

/// The `[exceptions].disabled_apps` list PolterType shipped as a
/// default up to and including v0.4.1, frozen verbatim.
///
/// Here for exactly one reason: to be *recognised* and retired. Every
/// config written by those versions spells these 69 entries out, and on
/// Linux they became load-bearing the moment the focus tracker landed,
/// muting the app in every editor its owner uses.
/// `retire_default_skip_list` clears the list only when it still
/// matches this exactly — so it has to stay byte-for-byte accurate, or
/// we either miss the configs we mean to fix or clobber a list somebody
/// wrote themselves.
///
/// Nothing reads this to *apply* it. The default today is empty.
pub(crate) const LEGACY_DEFAULT_DISABLED_APPS: [&str; 69] = [
    "Code.exe",
    "code",
    "Code - Insiders.exe",
    "code-insiders",
    "Cursor.exe",
    "cursor",
    "Cursor",
    "idea64.exe",
    "idea.exe",
    "idea",
    "rustrover64.exe",
    "rustrover",
    "pycharm64.exe",
    "pycharm",
    "webstorm64.exe",
    "webstorm",
    "clion64.exe",
    "clion",
    "goland64.exe",
    "goland",
    "phpstorm64.exe",
    "phpstorm",
    "rider64.exe",
    "rider",
    "datagrip64.exe",
    "datagrip",
    "android-studio.exe",
    "android-studio",
    "fleet.exe",
    "fleet",
    "sublime_text.exe",
    "sublime_text",
    "Sublime Text",
    "Notepad++.exe",
    "Zed.exe",
    "zed",
    "Zed",
    "neovide.exe",
    "neovide",
    "gvim.exe",
    "gvim",
    "nvim-qt.exe",
    "emacs.exe",
    "WindowsTerminal.exe",
    "wt.exe",
    "powershell.exe",
    "pwsh.exe",
    "cmd.exe",
    "ConEmu64.exe",
    "ConEmu.exe",
    "tabby.exe",
    "tabby",
    "alacritty.exe",
    "alacritty",
    "wezterm-gui.exe",
    "wezterm",
    "kitty.exe",
    "kitty",
    "konsole",
    "gnome-terminal",
    "gnome-terminal-server",
    "xterm",
    "tilix",
    "Terminal", // macOS Terminal.app,
    "iTerm2",
    "git-bash.exe",
    "mintty.exe",
    "tmux",
    "screen",
];
