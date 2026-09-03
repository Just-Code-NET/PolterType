//! Text for the command-line entry points `main` dispatches on.

use crate::consts::APP_NAME;

pub(crate) fn print_help() {
    println!(
        "{APP_NAME} {ver}\n\
        \n\
        USAGE:\n  \
            poltertype              start the tray app\n  \
            poltertype --settings   open the settings window\n  \
            poltertype --setup      open the settings window on the Setup pane\n  \
            poltertype --version    print version and exit\n  \
            poltertype --help       show this help",
        ver = env!("CARGO_PKG_VERSION"),
    );
}
