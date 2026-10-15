//! Schema versioning for `config.toml`, the frozen historical defaults
//! a migration has to recognise, and other fixed settings values.

pub(crate) const SCHEMA_VERSION: u32 = 1;

/// Never check more often than this, whatever `config.toml` says.
pub const MIN_UPDATE_INTERVAL_HOURS: u64 = 1;

/// `[suggestions].window_margin` out of the box: the gap the tooltip
/// always kept above the window's bottom edge.
pub const TOOLTIP_DEFAULT_MARGIN: u32 = 96;
/// Largest `[suggestions].window_margin` honoured.
pub const TOOLTIP_MAX_MARGIN: u32 = 400;
/// `[suggestions].scale_percent` bounds: half size is the smallest
/// still readable, three times fills a 4K panel's corner.
pub const TOOLTIP_MIN_SCALE_PERCENT: u32 = 50;
pub const TOOLTIP_MAX_SCALE_PERCENT: u32 = 300;

/// The `[exceptions].disabled_apps` list PolterType shipped as a
/// default up to and including v0.4.1, frozen verbatim.
///
/// Here to be *recognised* and retired, never applied — the default
/// today is empty. `retire_default_skip_list` clears a user's list only
/// when it still matches this exactly, so it must stay byte-for-byte
/// accurate: otherwise we miss the configs we mean to fix, or clobber a
/// list somebody wrote themselves.
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
