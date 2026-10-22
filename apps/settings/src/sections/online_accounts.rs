//! Connected online accounts via GNOME Online Accounts.

use crate::widgets::{self, Page};
use crate::{cmd, window};
use gtk::prelude::*;
use nixe_accounts::{self as goa, Account, Error, ServiceState};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

enum Screen {
    List,
    Detail(String),
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
        Screen::Detail(id) => Screen::Detail(id.clone()),
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
        Screen::Detail(id) => detail(page, id),
        Screen::Add => add_page(page),
        Screen::Wait => wait_page(page),
        Screen::List => list(page),
    }
}

fn list(page: &Page) {
    let g = page.group("Accounts");
    g.note("Google, Microsoft, Nextcloud and other accounts this computer uses for mail, calendar, contacts and files.");
    let holder = widgets::vbox(0);
    holder.append(&widgets::label("Looking for accounts…", "dim"));
    g.add(&holder);
    cmd::background(
        || {
            let client = goa::Client::connect().map_err(|_| Error::Unavailable)?;
            let accounts = client.list()?;
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
    accounts: Vec<Account>,
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
                g.top(&widgets::banner(
                    "<b>The keyring isn't available.</b> Online accounts cannot be saved.",
                    true,
                ));
            }
            if !goa::backend_available() && !goa::helper_available() {
                g.top(&widgets::banner(
                    "<b>Can't add accounts from here.</b> Install gnome-online-accounts so Settings can sign in with the browser.",
                    true,
                ));
            }
            if accounts.is_empty() {
                holder.append(&widgets::label("No online accounts.", "dim"));
            } else {
                for account in &accounts {
                    holder.append(&row(account));
                }
            }
            let (r, _) = widgets::button_row(
                "Add account",
                "Choose Google, Microsoft, Nextcloud or another provider, then sign in with your browser.",
                "Add",
                |_| show(Screen::Add),
            );
            widgets::keywords("add connect sign in google microsoft nextcloud webdav oauth");
            holder.append(&r);
        }
    }
}

fn row(account: &Account) -> gtk::Box {
    let services = service_line(&account.services);
    let desc = match (account.provider_name.as_str(), services.as_str()) {
        ("", "") => String::new(),
        (p, "") => p.to_string(),
        ("", s) => s.to_string(),
        (p, s) => format!("{p} · {s}"),
    };
    let desc = gtk::glib::markup_escape_text(&desc).to_string();
    let controls = widgets::hbox(6);
    let status = gtk::Label::new(Some(account.status().label()));
    status.add_css_class("dim");
    if account.attention_needed {
        status.add_css_class("danger");
    }
    controls.append(&status);
    let open = gtk::Button::with_label("Open");
    let id = account.id.clone();
    open.connect_clicked(move |_| show(Screen::Detail(id.clone())));
    controls.append(&open);
    let r = widgets::row(account.label(), &desc, Some(controls.upcast_ref()));
    widgets::keywords(&format!(
        "{} {} {} google microsoft nextcloud webdav oauth",
        account.provider_type, account.identity, account.presentation
    ));
    r
}

fn detail(page: &Page, id: String) {
    let (back, _) = widgets::button_row("All accounts", "", "Back", |_| show(Screen::List));
    let top = page.group("");
    top.add(&back);

    let info = page.group("Account");
    let holder = widgets::vbox(0);
    holder.append(&widgets::label("Looking for this account…", "dim"));
    info.add(&holder);

    cmd::background(
        move || {
            let client = goa::Client::connect().map_err(|_| Error::Unavailable)?;
            client.get(&id)
        },
        {
            let holder = holder.clone();
            let info = info.clone();
            let page_services = page.body.clone();
            move |found| fill_detail(&info, &holder, &page_services, found)
        },
    );
}

fn fill_detail(info: &widgets::Group, holder: &gtk::Box, page_body: &gtk::Box, found: Result<Option<Account>, Error>) {
    widgets::begin_section("online-accounts");
    widgets::forget_rows(holder);
    while let Some(c) = holder.first_child() {
        holder.remove(&c);
    }
    let account = match found {
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

    let (r, _) = widgets::info_row("Account", account.label());
    holder.append(&r);
    if !account.provider_name.is_empty() {
        let (r, _) = widgets::info_row("Provider", &account.provider_name);
        holder.append(&r);
    }
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

    let services: Vec<ServiceState> = account.services.iter().filter(|s| s.can_toggle).cloned().collect();
    if !services.is_empty() {
        let g = group_on(page_body, "Services");
        g.note("Turning a service off stops this computer using it. Mail, events and files stay on the provider.");
        for svc in services {
            let path = account.path.clone();
            let service = svc.service.clone();
            let on = svc.enabled;
            let (r, _) = widgets::switch_row(svc.service.label(), &used_by(&svc.service), on, move |now| {
                if now == on {
                    return;
                }
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

    let g = group_on(page_body, "Remove");
    g.note("This disconnects the account from this computer. Mail, events, contacts, and files on the provider are not deleted.");
    if account.locked {
        g.add(&widgets::label("This account is locked, so it can't be removed here.", "dim"));
    } else {
        let path = account.path.clone();
        let b = widgets::confirm_button("Remove", "Remove this account?", move |_| {
            let path = path.clone();
            cmd::background(
                move || {
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
    let (back, _) = widgets::button_row("All accounts", "", "Back", |_| show(Screen::List));
    page.group("").add(&back);
    let g = page.group("Add account");
    g.note("Sign in with your browser. Google and Microsoft will still say GNOME is asking for access.");
    let holder = widgets::vbox(0);
    holder.append(&widgets::label("Looking for providers…", "dim"));
    g.add(&holder);
    cmd::background(goa::list_providers, {
        let holder = holder.clone();
        move |list| fill_providers(&holder, list)
    });
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
                holder.append(&widgets::banner(
                    "<b>Can't list providers.</b> Install gnome-online-accounts.",
                    true,
                ));
            }
        }
        Err(e) => holder.append(&widgets::banner(&glib_markup(&e.to_string()), true)),
        Ok(providers) if providers.is_empty() => {
            holder.append(&widgets::label("No providers on this computer.", "dim"));
        }
        Ok(providers) => {
            for p in providers {
                let ty = p.provider_type.clone();
                let (r, _) = widgets::button_row(&p.name, "", "Add", move |_| start_job(&["add", &ty]));
                widgets::keywords(&format!("{} google microsoft nextcloud webdav imap oauth", p.provider_type));
                holder.append(&r);
            }
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
    match gtk::gio::Subprocess::newv(&refs, gtk::gio::SubprocessFlags::STDERR_PIPE) {
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

/// The installed Nixe app that uses a service, for its switch.
fn used_by(service: &goa::Service) -> String {
    let app = match service {
        goa::Service::Mail => ("nixe-mail", "Nixe Mail"),
        goa::Service::Calendar => ("nixe-calendar", "Nixe Calendar"),
        goa::Service::Tasks => ("nixe-reminders", "Nixe Reminders"),
        goa::Service::Contacts => ("nixe-contacts", "Nixe Contacts"),
        _ => return String::new(),
    };
    if goa::find_bin(app.0).is_some() { format!("Used by {}", app.1) } else { String::new() }
}

/// What Nixe Mail and Calendar can and can't do with a Microsoft 365 account.
fn microsoft_note(account: &Account) -> Option<String> {
    if account.provider_type != "ms_graph" {
        return None;
    }
    let mut parts = Vec::new();
    if goa::find_bin("nixe-mail").is_some() {
        parts.push("Nixe Mail can't use Microsoft 365 mail yet.".to_string());
    }
    let modules = std::path::Path::new("/usr/lib/evolution-data-server/registry-modules");
    let ews = ["module-microsoft365-backend.so", "module-ews-backend.so"].iter().any(|m| modules.join(m).exists());
    if !ews && (goa::find_bin("nixe-calendar").is_some() || goa::find_bin("nixe-reminders").is_some()) {
        parts.push("Its calendars and task lists need <b>evolution-ews</b> installed.".to_string());
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn service_line(services: &[ServiceState]) -> String {
    services.iter().filter(|s| s.enabled).map(|s| s.service.label()).collect::<Vec<_>>().join(", ")
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
