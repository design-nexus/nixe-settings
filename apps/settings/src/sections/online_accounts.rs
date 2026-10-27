//! Connected online accounts via GNOME Online Accounts.

use super::account_icons::{self, Logo};
use super::online_presets::Preset;
use crate::widgets::{self, Page};
use crate::{cmd, theme, window};
use gtk::glib::variant::ToVariant;
use gtk::prelude::*;
use nyx_accounts::{self as goa, Account, Error, ServiceState};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

enum Screen {
    List,
    Add,
    Wait,
}

thread_local! {
    static SCREEN: RefCell<Screen> = const { RefCell::new(Screen::List) };
    static JOB: RefCell<Option<gtk::gio::Subprocess>> = const { RefCell::new(None) };
}

fn screen() -> Screen {
    SCREEN.with(|s| match &*s.borrow() {
        Screen::List => Screen::List,
        Screen::Add => Screen::Add,
        Screen::Wait => Screen::Wait,
    })
}

fn show(next: Screen) {
    SCREEN.with(|s| *s.borrow_mut() = next);
    window::rebuild("online-accounts");
}

pub fn build(page: &Page) {
    watch_once();
    match screen() {
        Screen::Add => add_page(page),
        Screen::Wait => wait_page(page),
        Screen::List => {
            list(page);
            super::cloud::build(page);
        }
    }
}

/// A provider on the Add account page.
struct Provider {
    name: &'static str,
    desc: &'static str,
    logo: Logo,
    kind: Kind,
}

enum Kind {
    Goa(&'static str),
    Preset(Preset),
}

/// The Add account page, in this order.
const PROVIDERS: &[Provider] = &[
    Provider { name: "Google", desc: "Gmail, Calendar, Contacts and Drive", logo: Logo::Google, kind: Kind::Goa("google") },
    Provider {
        name: "Microsoft 365",
        desc: "Outlook calendar, contacts and OneDrive",
        logo: Logo::Microsoft,
        kind: Kind::Goa("ms_graph"),
    },
    Provider {
        name: "Fastmail",
        desc: "Mail, calendar and contacts with an app password",
        logo: Logo::Fastmail,
        kind: Kind::Preset(Preset::Fastmail),
    },
    Provider {
        name: "Nextcloud",
        desc: "Files, calendar and contacts on your server",
        logo: Logo::Nextcloud,
        kind: Kind::Goa("owncloud"),
    },
    Provider { name: "Zoho", desc: "Zoho Mail, Calendar and Contacts", logo: Logo::Zoho, kind: Kind::Preset(Preset::Zoho) },
    Provider { name: "IMAP and SMTP", desc: "Any other mail provider", logo: Logo::Mail, kind: Kind::Goa("imap_smtp") },
    Provider { name: "WebDAV", desc: "Calendars, contacts and files on a server", logo: Logo::WebDav, kind: Kind::Goa("webdav") },
];

/// GOA providers left off the Add account page.
const HIDDEN: &[&str] = &["kerberos", "exchange"];

fn account_icon(account: &Account, brand: Option<Preset>, size: i32) -> gtk::Image {
    let logo = brand.map(Logo::from).unwrap_or_else(|| account_icons::logo_for_goa(&account.provider_type));
    account_icons::image(logo, size)
}

/// Fastmail and Zoho are IMAP and WebDAV accounts to GOA; their servers say which.
fn brand_of(conn: &gtk::gio::DBusConnection, account: &Account) -> Option<Preset> {
    let lookups: &[(&str, &str)] = match account.provider_type.as_str() {
        "imap_smtp" => &[("org.gnome.OnlineAccounts.Mail", "ImapHost")],
        "webdav" => &[
            ("org.gnome.OnlineAccounts.Calendar", "Uri"),
            ("org.gnome.OnlineAccounts.Contacts", "Uri"),
            ("org.gnome.OnlineAccounts.Files", "Uri"),
        ],
        _ => return None,
    };
    let ty = gtk::glib::VariantTy::new("(v)").expect("reply type");
    lookups.iter().find_map(|(iface, prop)| {
        let reply = conn
            .call_sync(
                Some(goa::BUS_NAME),
                &account.path,
                "org.freedesktop.DBus.Properties",
                "Get",
                Some(&(*iface, *prop).to_variant()),
                Some(ty),
                gtk::gio::DBusCallFlags::NONE,
                3000,
                gtk::gio::Cancellable::NONE,
            )
            .ok()?;
        let host = reply.child_value(0).as_variant()?.str()?.to_string();
        Preset::for_host(&host)
    })
}

fn provider_label(account: &Account, brand: Option<Preset>) -> String {
    brand.map(|b| b.name().to_string()).unwrap_or_else(|| account.provider_name.clone())
}

/// Back to the account list (after adding one elsewhere).
pub fn show_list() {
    show(Screen::List);
}

fn list(page: &Page) {
    let g = page.group("Accounts");
    g.note("Mail, calendar, contacts and files from your online accounts.");
    let holder = widgets::vbox(0);
    holder.append(&widgets::label("Looking for accounts…", "dim"));
    g.add(&holder);
    cmd::background(
        || {
            let client = goa::Client::connect().map_err(|_| Error::Unavailable)?;
            let accounts = client
                .list()?
                .into_iter()
                .map(|a| {
                    let brand = brand_of(client.connection(), &a);
                    (a, brand)
                })
                .collect();
            Ok(Listed { accounts, keyring: goa::keyring_available() })
        },
        {
            let holder = holder.clone();
            let g = g.clone();
            move |listed| fill_list(&g, &holder, listed)
        },
    );
}

struct Listed {
    accounts: Vec<(Account, Option<Preset>)>,
    keyring: bool,
}

fn fill_list(g: &widgets::Group, holder: &gtk::Box, listed: Result<Listed, Error>) {
    widgets::begin_section("online-accounts");
    widgets::forget_rows(holder);
    while let Some(c) = holder.first_child() {
        holder.remove(&c);
    }
    match listed {
        Err(Error::Unavailable) => {
            g.top(&widgets::banner(
                "<b>Online Accounts isn't installed.</b> Install gnome-online-accounts to connect Google, Microsoft and Nextcloud.",
                true,
            ));
            holder.append(&widgets::label("No accounts to show until the service is installed.", "dim"));
        }
        Err(e) => {
            holder.append(&widgets::banner(&glib_markup(&e.to_string()), true));
        }
        Ok(Listed { accounts, keyring }) => {
            if !keyring {
                g.top(&widgets::banner("<b>The keyring isn't available.</b> Online accounts cannot be saved.", true));
            }
            if !goa::backend_available() && !goa::helper_available() {
                g.top(&widgets::banner(
                    "<b>Can't add accounts from here.</b> Install gnome-online-accounts so Settings can sign in with the browser.",
                    true,
                ));
            }
            if accounts.is_empty() {
                let empty = widgets::row("No accounts yet", "Add one to use its mail, calendar, contacts and files.", None);
                empty.prepend(&account_icons::image(Logo::Generic, 32));
                holder.append(&empty);
            } else {
                for (account, brand) in &accounts {
                    holder.append(&row(account, *brand));
                }
            }
            let (r, b) =
                widgets::button_row("Add account", "Google, Microsoft 365, Fastmail, Nextcloud, Zoho and more", "Add…", |_| {
                    show(Screen::Add)
                });
            b.add_css_class("suggested-action");
            r.add_css_class("add-account-row");
            widgets::keywords("add connect sign in google microsoft fastmail nextcloud zoho imap webdav oauth");
            holder.append(&r);
        }
    }
}

fn row(account: &Account, brand: Option<Preset>) -> gtk::Box {
    let services = service_line(&account.services);
    let provider = provider_label(account, brand);
    let desc = match (provider.as_str(), services.as_str()) {
        ("", "") => String::new(),
        (p, "") => p.to_string(),
        ("", s) => s.to_string(),
        (p, s) => format!("{p} · {s}"),
    };
    let desc = gtk::glib::markup_escape_text(&desc).to_string();
    let controls = widgets::hbox(6);
    let status = widgets::tag(account.status().label());
    status.add_css_class(if account.attention_needed { "warn-tag" } else { "ok-tag" });
    controls.append(&status);
    let open = widgets::icon_button("emblem-system-symbolic", "Account settings");
    let id = account.id.clone();
    open.connect_clicked(move |_| account_popup(id.clone()));
    controls.append(&open);
    let r = widgets::row(account.label(), &desc, Some(controls.upcast_ref()));
    r.add_css_class("account-row");
    r.prepend(&account_icon(account, brand, 32));
    widgets::keywords(&format!(
        "{} {} {} {} google microsoft fastmail nextcloud zoho webdav oauth",
        account.provider_type, account.identity, account.presentation, provider
    ));
    r
}

fn account_popup(id: String) {
    thread_local! {static OPEN: RefCell<std::collections::HashMap<String,gtk::glib::WeakRef<gtk::Window>>> = RefCell::new(std::collections::HashMap::new());}
    if let Some(w) = OPEN.with(|s| s.borrow().get(&id).and_then(|w| w.upgrade())) {
        w.present();
        return;
    }
    let (popup, body) = super::cloud::dialog("Account settings");
    OPEN.with(|s| {
        s.borrow_mut().insert(id.clone(), popup.downgrade());
    });
    let page = Page { root: gtk::ScrolledWindow::new(), body };
    let info = page.group("Account");
    let holder = widgets::vbox(0);
    holder.append(&widgets::label("Looking for this account…", "dim"));
    info.add(&holder);

    cmd::background(
        move || {
            let client = goa::Client::connect().map_err(|_| Error::Unavailable)?;
            Ok(client.get(&id)?.map(|a| {
                let brand = brand_of(client.connection(), &a);
                (a, brand)
            }))
        },
        {
            let holder = holder.clone();
            let info = info.clone();
            let page_services = page.body.clone();
            move |found| fill_detail(&info, &holder, &page_services, found)
        },
    );
}

fn fill_detail(
    info: &widgets::Group,
    holder: &gtk::Box,
    page_body: &gtk::Box,
    found: Result<Option<(Account, Option<Preset>)>, Error>,
) {
    widgets::begin_section("online-accounts");
    widgets::forget_rows(holder);
    while let Some(c) = holder.first_child() {
        holder.remove(&c);
    }
    let (account, brand) = match found {
        Err(e) => {
            holder.append(&widgets::banner(&glib_markup(&e.to_string()), true));
            return;
        }
        Ok(None) => {
            show(Screen::List);
            holder.append(&widgets::label("This account isn't here.", "dim"));
            return;
        }
        Ok(Some(a)) => a,
    };

    if account.attention_needed {
        info.top(&widgets::banner(
            "<b>This account needs you to sign in again.</b> Mail, calendar and files will wait until you do.",
            true,
        ));
    }
    if let Some(text) = microsoft_note(&account) {
        info.top(&widgets::banner(&text, false));
    }

    let provider = gtk::glib::markup_escape_text(&provider_label(&account, brand)).to_string();
    let head = widgets::row(account.label(), &provider, None);
    head.add_css_class("account-head");
    head.prepend(&account_icon(&account, brand, 40));
    holder.append(&head);
    let (r, _) = widgets::info_row("Status", account.status().label());
    holder.append(&r);

    let (r, _) = widgets::button_row(
        "Sign in again",
        "Opens the browser when this account's password or sign-in has expired. Google and Microsoft still name GNOME as the app.",
        "Sign in",
        {
            let id = account.id.clone();
            move |_| start_job(&["refresh", &id])
        },
    );
    widgets::keywords("reauth sign in again oauth expired");
    holder.append(&r);

    let services: Vec<ServiceState> = account
        .services
        .iter()
        .filter(|s| {
            s.can_toggle
                && s.service != goa::Service::Files
                && !(account.provider_type == "ms_graph" && s.service == goa::Service::Mail)
        })
        .cloned()
        .collect();
    if !services.is_empty() {
        let g = group_on(page_body, "Services");
        g.note("Turning a service off stops this computer using it. Mail, events and files stay on the provider.");
        for svc in services {
            let path = account.path.clone();
            let service = svc.service.clone();
            let on = svc.enabled;
            let (r, _) = widgets::switch_row(svc.service.label(), &used_by(&svc.service), on, move |now| {
                let (path, service) = (path.clone(), service.clone());
                cmd::background(
                    move || {
                        let client = goa::Client::connect().map_err(|_| Error::Unavailable)?;
                        client.set_service_enabled(&path, &service, now)
                    },
                    |r| {
                        if let Err(e) = r {
                            window::toast(&e.to_string());
                            window::rebuild("online-accounts");
                        }
                    },
                );
            });
            widgets::keywords(&format!("{} mail calendar contacts files", svc.service.label()));
            g.add(&r);
        }
    }

    if account.provider_type == "ms_graph" {
        let support = group_on(page_body, "Microsoft services");
        support.note("Mail and Tasks are not supported for this account yet. Calendar and Contacts use Evolution Data Server.");
        if !std::path::Path::new("/usr/lib/evolution-data-server/registry-modules/module-microsoft365-backend.so").exists() {
            let (row, _) = widgets::button_row(
                "Calendar and Contacts support",
                "Install evolution-ews. System authorization is required.",
                "Install support",
                |_| {
                    cmd::background(
                        || {
                            let status = std::process::Command::new("pkexec")
                                .args(["/usr/bin/pacman", "-S", "--needed", "--noconfirm", "evolution-ews"])
                                .status()?;
                            anyhow::ensure!(status.success(), "Installation did not complete");
                            anyhow::Ok(())
                        },
                        |result| match result {
                            Ok(()) => window::toast("Support installed. Sign out and back in to load the new account backend."),
                            Err(_) => window::toast("Support was not installed. You can retry from this account's settings."),
                        },
                    );
                },
            );
            support.add(&row);
        }
    }
    super::cloud::append_for_account(page_body, &account.id);

    let g = group_on(page_body, "Remove");
    g.note("This disconnects the account from this computer. Mail, events, contacts, and files on the provider are not deleted.");
    if account.locked {
        g.add(&widgets::label("This account is locked, so it can't be removed here.", "dim"));
    } else {
        let path = account.path.clone();
        let account_id = account.id.clone();
        let b = widgets::confirm_button("Remove", "Remove this account?", move |_| {
            let path = path.clone();
            let account_id = account_id.clone();
            cmd::background(
                move || {
                    if let Ok(cloud) = nyx_cloud::Client::connect() {
                        let remotes = cloud.list().map_err(|_| Error::Unavailable)?;
                        for r in remotes.into_iter().filter(|r| r.account_id.as_deref() == Some(&account_id)) {
                            cloud.remove(&r.id, r.managed).map_err(|_| Error::Unavailable)?;
                        }
                    }
                    let client = goa::Client::connect().map_err(|_| Error::Unavailable)?;
                    client.remove(&path)
                },
                |r| match r {
                    Ok(()) => {
                        window::toast("Account disconnected from this computer");
                        show(Screen::List);
                    }
                    Err(e) => window::toast(&e.to_string()),
                },
            );
        });
        let r = widgets::row("Remove from this computer", "", Some(b.upcast_ref()));
        widgets::keywords("remove disconnect delete account");
        g.add(&r);
    }
}

/// Extra groups after the page was already built (detail fills in later).
fn group_on(body: &gtk::Box, title: &str) -> widgets::Group {
    // Page::group appends to page.body; rebuild a matching group on the same page.
    let page = Page { root: gtk::ScrolledWindow::new(), body: body.clone() };
    page.group(title)
}

fn add_page(page: &Page) {
    let back = widgets::labeled_button("go-previous-symbolic", "All accounts");
    back.add_css_class("flat");
    back.add_css_class("back-button");
    back.set_halign(gtk::Align::Start);
    back.connect_clicked(|_| show(Screen::List));
    page.body.append(&back);
    let g = page.plain_group("Add account");
    g.note("Choose a provider, then sign in. Google and Microsoft still name GNOME as the app asking.");
    let holder = widgets::vbox(0);
    holder.append(&widgets::label("Looking for providers…", "dim"));
    g.add(&holder);
    cmd::background(goa::list_providers, {
        let holder = holder.clone();
        move |list| fill_providers(&holder, list)
    });
}

fn provider_card(name: &str, desc: &str, logo: Logo, on_click: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class("picture-card");
    b.add_css_class("add-account");
    let c = widgets::vbox(6);
    let i = account_icons::image(logo, 48);
    i.set_halign(gtk::Align::Center);
    c.append(&i);
    let t = widgets::label(name, "picture-card-title");
    t.set_halign(gtk::Align::Center);
    c.append(&t);
    let d = widgets::label(desc, "picture-card-label");
    d.set_wrap(true);
    d.set_justify(gtk::Justification::Center);
    d.set_halign(gtk::Align::Center);
    d.set_max_width_chars(24);
    c.append(&d);
    b.set_child(Some(&c));
    b.set_tooltip_text(Some(&format!("Add a {name} account")));
    b.connect_clicked(move |_| on_click());
    b
}

fn fill_providers(holder: &gtk::Box, list: Result<Vec<goa::ProviderInfo>, Error>) {
    widgets::begin_section("online-accounts");
    widgets::forget_rows(holder);
    while let Some(c) = holder.first_child() {
        holder.remove(&c);
    }
    match list {
        Err(Error::Unavailable) => {
            if goa::helper_available() {
                let (r, _) = widgets::button_row(
                    "Use the Online Accounts window",
                    "The native helper isn't here. The standalone window can still add an account.",
                    "Open",
                    |_| match goa::open_management() {
                        Ok(()) => window::toast("Finish in the Online Accounts window."),
                        Err(e) => window::toast(&format!("{e}")),
                    },
                );
                holder.append(&r);
            } else {
                holder.append(&widgets::banner("<b>Can't list providers.</b> Install gnome-online-accounts.", true));
            }
        }
        Err(e) => holder.append(&widgets::banner(&glib_markup(&e.to_string()), true)),
        Ok(providers) if providers.is_empty() => {
            holder.append(&widgets::label("No providers on this computer.", "dim"));
        }
        Ok(providers) => {
            let has = |ty: &str| providers.iter().any(|p| p.provider_type == ty);
            let flow = gtk::FlowBox::new();
            flow.set_selection_mode(gtk::SelectionMode::None);
            flow.set_homogeneous(true);
            flow.set_min_children_per_line(2);
            flow.set_max_children_per_line(3);
            flow.set_column_spacing(10);
            flow.set_row_spacing(10);
            flow.add_css_class("picture-cards");
            flow.add_css_class("provider-cards");
            for p in PROVIDERS {
                match p.kind {
                    Kind::Goa(ty) if has(ty) => {
                        flow.append(&provider_card(p.name, p.desc, p.logo, move || start_job(&["add", ty])));
                    }
                    Kind::Preset(preset) if has("imap_smtp") => {
                        flow.append(&provider_card(p.name, p.desc, p.logo, move || super::online_presets::open(preset)));
                    }
                    _ => {}
                }
            }
            let known = |ty: &str| PROVIDERS.iter().any(|p| matches!(p.kind, Kind::Goa(t) if t == ty));
            for p in providers.iter().filter(|p| !known(&p.provider_type) && !HIDDEN.contains(&p.provider_type.as_str())) {
                let ty = p.provider_type.clone();
                flow.append(&provider_card(&p.name, "", Logo::Generic, move || start_job(&["add", &ty])));
            }
            holder.append(&flow);
        }
    }
}

fn wait_page(page: &Page) {
    let g = page.group("Sign in");
    g.note("Finish in your browser. This page waits until the account is saved.");
    let (r, _) = widgets::button_row("Cancel", "Stop waiting. The browser window may still be open.", "Cancel", |_| {
        cancel_job();
        show(Screen::List);
    });
    g.add(&r);
}

fn start_job(args: &[&str]) {
    cancel_job();
    let bin = match goa::find_bin(goa::BACKEND) {
        Some(p) => p,
        None => {
            match goa::open_management() {
                Ok(()) => window::toast("Finish in the Online Accounts window."),
                Err(e) => window::toast(&format!("{e}")),
            }
            return;
        }
    };
    let mut argv: Vec<std::ffi::OsString> = vec![bin.into_os_string()];
    argv.extend(args.iter().map(|a| std::ffi::OsString::from(*a)));
    let refs: Vec<&std::ffi::OsStr> = argv.iter().map(|s| s.as_os_str()).collect();
    let launcher = gtk::gio::SubprocessLauncher::new(gtk::gio::SubprocessFlags::STDERR_PIPE);
    let palette = theme::palette();
    let css = crate::paths::cache_dir().join("goa-dialog.css");
    let written = std::fs::create_dir_all(crate::paths::cache_dir())
        .and_then(|_| std::fs::write(&css, theme::adwaita_css(&palette, &theme::look())));
    if written.is_ok() {
        launcher.setenv("NYX_GOA_CSS", &css, true);
    }
    launcher.setenv("ADW_DEBUG_COLOR_SCHEME", if palette.light { "prefer-light" } else { "prefer-dark" }, true);
    match launcher.spawn(&refs) {
        Ok(proc) => {
            JOB.with(|j| *j.borrow_mut() = Some(proc.clone()));
            SCREEN.with(|s| *s.borrow_mut() = Screen::Wait);
            window::rebuild("online-accounts");
            let watched = proc.clone();
            proc.wait_async(gtk::gio::Cancellable::NONE, move |r| {
                JOB.with(|j| j.borrow_mut().take());
                match r {
                    Ok(()) if watched.is_successful() => window::toast("Account saved."),
                    Ok(()) => {}
                    Err(e) => window::toast(&e.to_string()),
                }
                show(Screen::List);
            });
        }
        Err(e) => window::toast(&e.to_string()),
    }
}

fn cancel_job() {
    JOB.with(|j| {
        if let Some(p) = j.borrow_mut().take() {
            p.force_exit();
        }
    });
}

/// The installed Nyx app that uses a service, for its switch.
fn used_by(service: &goa::Service) -> String {
    let app = match service {
        goa::Service::Mail => ("nyx-mail", "Mail"),
        goa::Service::Calendar => ("nyx-calendar", "Calendar"),
        goa::Service::Tasks => ("nyx-reminders", "Reminders"),
        goa::Service::Contacts => ("nyx-contacts", "Contacts"),
        _ => return String::new(),
    };
    if goa::find_bin(app.0).is_some() { format!("Used by {}", app.1) } else { String::new() }
}

/// What Nyx Mail and Calendar can and can't do with a Microsoft 365 account.
fn microsoft_note(account: &Account) -> Option<String> {
    if account.provider_type != "ms_graph" {
        return None;
    }
    let mut parts = Vec::new();
    if goa::find_bin("nyx-mail").is_some() {
        parts.push("Mail can't use Microsoft 365 mail yet.".to_string());
    }
    let modules = std::path::Path::new("/usr/lib/evolution-data-server/registry-modules");
    let ews = ["module-microsoft365-backend.so", "module-ews-backend.so"].iter().any(|m| modules.join(m).exists());
    if !ews
        && (goa::find_bin("nyx-calendar").is_some()
            || goa::find_bin("nyx-reminders").is_some()
            || goa::find_bin("nyx-contacts").is_some())
    {
        parts.push("Calendar and Contacts need <b>evolution-ews</b> installed.".to_string());
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn service_line(services: &[ServiceState]) -> String {
    services
        .iter()
        .filter(|s| s.enabled && !matches!(s.service, goa::Service::Unknown(_)))
        .map(|s| s.service.label())
        .collect::<Vec<_>>()
        .join(", ")
}

fn glib_markup(text: &str) -> String {
    format!("<b>{}</b>", text.replace('&', "&amp;").replace('<', "&lt;"))
}

fn watch_once() {
    thread_local! {
        static WATCHER: RefCell<Option<goa::Client>> = const { RefCell::new(None) };
    }
    WATCHER.with(|slot| {
        if slot.borrow().is_some() {
            return;
        }
        let Ok(client) = goa::Client::connect() else {
            return;
        };
        let pending = Rc::new(Cell::new(false));
        client.watch(move || {
            if pending.replace(true) {
                return;
            }
            let pending = pending.clone();
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                pending.set(false);
                window::rebuild_if_built("online-accounts");
            });
        });
        *slot.borrow_mut() = Some(client);
    });
}
