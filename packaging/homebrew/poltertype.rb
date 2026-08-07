# Homebrew cask for PolterType.
#
# Goes into Just-Code-NET/homebrew-tap as `Casks/poltertype.rb`, which
# makes the install line:
#
#     brew install --cask just-code-net/tap/poltertype
#
# See ../README.md for creating the tap and keeping this file current.
# `bin/bump-cask.sh` in that tap rewrites `version` and `sha256` from a
# release; do not hand-edit them.

cask "poltertype" do
  version "0.14.2"
  sha256 "1cf73316e27515182e01d13147d999d6d01726504a576b2b91eb92f1c7fb2747"

  url "https://github.com/Just-Code-NET/PolterType/releases/download/v#{version}/poltertype-#{version}-universal-apple-darwin.dmg",
      verified: "github.com/Just-Code-NET/PolterType/"
  name "PolterType"
  desc "Detects text typed in the wrong keyboard layout and fixes the word"
  homepage "https://poltertype.com/"

  livecheck do
    url :url
    strategy :github_latest
  end

  # The app owns a global keyboard hook, so it must not be running when
  # its bundle is replaced.
  depends_on macos: ">= :big_sur"

  app "poltertype.app"

  # NOT `quarantine: false`, and not an `xattr -d` in a postflight.
  #
  # PolterType's installers are unsigned and un-notarised today. A cask
  # that strips the quarantine flag would make first launch smooth by
  # removing the one check standing between the user and an unverified
  # binary — and it would do it silently, on their behalf, for an app
  # that reads every keystroke they type. The caveat below tells them
  # what to do instead and why, and leaves the decision where it
  # belongs. Delete this comment the day the DMG is notarised, not
  # before.
  caveats <<~EOS
    PolterType is not code-signed or notarised yet, so macOS will refuse
    the first launch with "the developer cannot be verified".

    To open it once:  right-click poltertype.app in /Applications and
    choose Open, then confirm.

    PolterType also needs two permissions before it can do anything, in
    System Settings > Privacy & Security:

      * Accessibility     — to read keys and type the correction
      * Input Monitoring  — to receive the key events at all

    Granting only one of the two is the usual reason it looks dead. The
    app's Settings window has a Setup pane that says which of the two
    is missing.
  EOS

  # Paths taken from the code, not guessed: `ProjectDirs::from("dev",
  # "opensource", "poltertype")` in poltertype-core's settings store,
  # and `APP_ID` = dev.opensource.poltertype for the LaunchAgent
  # (crates/poltertype-autostart/src/macos.rs). A `zap` that lists a
  # path the app never writes is a cask quietly claiming to clean up
  # after itself.
  zap trash: [
    "~/Library/Application Support/dev.opensource.poltertype",
    "~/Library/Caches/dev.opensource.poltertype",
    "~/Library/LaunchAgents/dev.opensource.poltertype.plist",
    "~/Library/Preferences/dev.opensource.poltertype.plist",
  ]
end
