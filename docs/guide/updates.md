# Software updates

- **About FastDB**: Open the application menu's About dialog to view the app icon, name and version; close it with **Close**, `Esc` or `Cmd/Ctrl + W`.
- **In-app update checks**: Queries GitHub Releases for new FastDB versions and compares semantic versioning. If the API is unavailable (including 403/rate limits), follows the public latest-release redirect as a fallback. When only the version is available, use **View Release** for notes and downloads.
- **Automatic startup check**: Throttled background check runs silently once every 24 hours on app launch.
- **Manual check via menu**: Check anytime via the application menu `FastDB -> Check for Updates...`.
- **Native macOS confirmation**: When already up to date, shows a system alert with the app icon, FastDB name, current version and **OK**, matching Litty's update confirmation.
- About and update status dialogs use compact, content-sized layouts; update dialogs with release notes have extra width for readability.
- **In-app download & progress**: View markdown release notes directly in a native modal, download update packages with real-time progress bars, and launch the installer (.dmg on macOS, packages on Linux).
- **Top bar indicator badge**: When a new release is available, a subtle notification pill appears in the window top bar for 1-click access.
