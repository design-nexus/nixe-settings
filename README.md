# Settings

Native Nixe application for Omarchy. Existing names, application IDs, data paths,
and integrations are preserved.

- Source and application documentation: [apps/settings](apps/settings).
- Build: `cargo build --release --locked --manifest-path Cargo.toml`.
- Install: `./install.sh` (preserves existing integration resources).
- Package: `python3 tools/package.py`; notes: `python3 tools/release-notes.py`.
- Development rules and public dependency boundaries: [AGENTS.md](AGENTS.md).

Shared dependencies are pinned to immutable GitHub revisions.
Use main for the standalone build; preserved tags describe historical source.

Online Accounts is the single account manager. Each account's Settings popup
selects its supported services, including Storage visibility in Files and optional
Markdown Notes synchronization. Existing rclone connections can be explicitly
imported without changing their original configuration. Additional storage OAuth
stays inside the account setup flow. Legacy `--section cloud-storage` opens
Online Accounts. Microsoft Mail/Tasks remain unavailable; the popup offers
installation of the EDS backend needed for Microsoft Calendar/Contacts.
