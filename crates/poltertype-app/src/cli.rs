//! Text for the command-line entry points `main` dispatches on.

use anyhow::{Context, Result, bail};

use crate::consts::APP_NAME;

pub(crate) fn print_help() {
    println!(
        "{APP_NAME} {ver}\n\
        \n\
        USAGE:\n  \
            poltertype                      start the tray app\n  \
            poltertype --settings           open the settings window\n  \
            poltertype --setup              open the settings window on the Setup pane\n  \
            poltertype --install-plugin DIR install a plug-in for this user\n  \
            poltertype --remove-plugin ID   remove a plug-in this user installed\n  \
            poltertype --plugin-strings ID  print a plug-in's translatable strings\n  \
            poltertype --version            print version and exit\n  \
            poltertype --help               show this help",
        ver = env!("CARGO_PKG_VERSION"),
    );
}

/// Print the catalog file a translator starts from for one plug-in:
/// every string its settings pane draws, under the key PolterType will
/// look it up by.
///
/// Derived from the installed manifest rather than documented as a
/// naming rule, so the answer cannot drift from what the loader does.
pub(crate) fn print_plugin_strings(id: &str) -> Result<()> {
    let data_dir = poltertype_core::data_dir::resolve().context("find the data directory")?;
    let found = poltertype_core::plugins::extensions(&data_dir)
        .into_iter()
        .find(|e| e.id == id);
    let Some(ext) = found else {
        let known: Vec<String> = poltertype_core::plugins::extensions(&data_dir)
            .into_iter()
            .map(|e| e.id)
            .collect();
        bail!(
            "no plug-in with id {id:?}{}",
            if known.is_empty() {
                " — none are installed".to_owned()
            } else {
                format!(" — installed: {}", known.join(", "))
            }
        );
    };

    println!("# {} {} — settings pane strings.", ext.name, ext.version);
    println!("#");
    println!(
        "# Save as {}/i18n/<lang>.toml and translate the right-hand",
        ext.dir.display()
    );
    println!("# side. Keys are relative to this plug-in: PolterType keeps them");
    println!("# under `plugin.{id}.`, so nothing here can reach its own labels.");
    println!("# An untranslated line can simply be left out.");
    println!();
    for (key, english) in poltertype_core::plugins::strings(&ext.manifest) {
        println!("{} = {}", quoted(&key), quoted(&english));
    }
    Ok(())
}

/// Install the plug-in in `dir` for this user, replacing any earlier
/// copy of the same id.
///
/// Per user rather than beside the bundled data: on a packaged or
/// AppImage install that directory belongs to root or is read-only.
pub(crate) fn install_plugin(dir: &str) -> Result<()> {
    let root = poltertype_core::plugins::user_root().context("find this user's data directory")?;
    let pack = poltertype_core::plugins::install(std::path::Path::new(dir), &root)
        .with_context(|| format!("install the plug-in in {dir}"))?;
    println!(
        "Installed {} {} into {}.",
        pack.name,
        pack.version,
        pack.path.display()
    );
    for skipped in &pack.skipped {
        println!("  not copied: {skipped}");
    }
    println!("Restart PolterType to start it.");
    Ok(())
}

/// Remove a plug-in this user installed.
pub(crate) fn remove_plugin(id: &str) -> Result<()> {
    let root = poltertype_core::plugins::user_root().context("find this user's data directory")?;
    if !poltertype_core::plugins::uninstall(id, &root)? {
        bail!("no plug-in {id:?} is installed for this user");
    }
    println!("Removed {id}. Restart PolterType to stop it.");
    Ok(())
}

/// A TOML basic string. Labels are one line of prose, so this covers
/// what actually needs covering rather than the whole escape table.
fn quoted(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}
