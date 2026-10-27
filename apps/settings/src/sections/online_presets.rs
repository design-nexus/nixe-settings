//! Fastmail and Zoho: mail, and optionally calendar and contacts, set up from
//! an email address and an app password, then saved through GOA as an IMAP and
//! SMTP account (and a WebDAV one). GOA has no provider of its own for either.

use crate::{cmd, widgets, window};
use gtk::glib::{self, variant::ToVariant};
use gtk::prelude::*;
use nyx_accounts as goa;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Fastmail,
    Zoho,
}

/// Zoho's data centres: (domain, label).
const ZOHO_REGIONS: &[(&str, &str)] = &[
    ("zoho.com", "United States (zoho.com)"),
    ("zoho.eu", "Europe (zoho.eu)"),
    ("zoho.in", "India (zoho.in)"),
    ("zoho.com.au", "Australia (zoho.com.au)"),
    ("zoho.jp", "Japan (zoho.jp)"),
    ("zohocloud.ca", "Canada (zohocloud.ca)"),
    ("zoho.sa", "Saudi Arabia (zoho.sa)"),
    ("zoho.com.cn", "China (zoho.com.cn)"),
];

impl Preset {
    pub fn name(self) -> &'static str {
        match self {
            Self::Fastmail => "Fastmail",
            Self::Zoho => "Zoho",
        }
    }

    /// The preset a mail or DAV server belongs to.
    pub fn for_host(host: &str) -> Option<Preset> {
        let host = host.to_ascii_lowercase();
        if host.contains("fastmail.") {
            Some(Self::Fastmail)
        } else if host.contains("zoho.") || host.contains("zohocloud.") {
            Some(Self::Zoho)
        } else {
            None
        }
    }

    fn password_page(self, region: &str) -> String {
        match self {
            Self::Fastmail => "https://app.fastmail.com/settings/security/apps".into(),
            Self::Zoho => format!("https://accounts.{region}/home#/security/app_password"),
        }
    }

    fn servers(self, email: &str, region: &str) -> Servers {
        match self {
            Self::Fastmail => Servers {
                imap: "imap.fastmail.com".into(),
                smtp: "smtp.fastmail.com".into(),
                caldav: "https://caldav.fastmail.com/dav/".into(),
                carddav: "https://carddav.fastmail.com/dav/".into(),
            },
            Self::Zoho => {
                // Free addresses are on zoho.*; custom domains are Zoho Workplace.
                let domain = email.rsplit('@').next().unwrap_or_default().to_ascii_lowercase();
                let personal = domain.starts_with("zoho.") || domain.starts_with("zohomail.");
                let pro = if personal { "" } else { "pro" };
                Servers {
                    imap: format!("imap{pro}.{region}"),
                    smtp: format!("smtp{pro}.{region}"),
                    caldav: format!("https://calendar.{region}/caldav/"),
                    carddav: format!("https://contacts.{region}/carddav/"),
                }
            }
        }
    }
}

struct Servers {
    imap: String,
    smtp: String,
    caldav: String,
    carddav: String,
}

struct Request {
    preset: Preset,
    name: String,
    email: String,
    password: String,
    region: String,
    dav: bool,
}

/// What came of saving: `Ok(None)` all went in, `Ok(Some(note))` mail went in
/// but calendar and contacts didn't.
type Outcome = Result<Option<String>, String>;

pub fn open(preset: Preset) {
    let Some(win) = window::window() else { return };
    let dialog =
        gtk::Window::builder().transient_for(&win).modal(true).title(format!("Add {}", preset.name())).default_width(440).build();
    dialog.add_css_class("settings-window");
    dialog.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    let card = widgets::vbox(10);
    card.add_css_class("dialog-card");
    card.add_css_class("preset-form");

    let head = widgets::hbox(12);
    let logo = super::account_icons::image(preset.into(), 40);
    head.append(&logo);
    let title = widgets::label(&format!("Add {}", preset.name()), "section-title");
    title.set_valign(gtk::Align::Center);
    head.append(&title);
    card.append(&head);
    let intro = widgets::label(
        &format!("Sign in with an app password from {}, not your usual password. It's saved in the keyring.", preset.name()),
        "dim",
    );
    intro.set_wrap(true);
    card.append(&intro);

    let name = gtk::Entry::new();
    name.set_placeholder_text(Some("Your name, as people see it"));
    let real = glib::real_name().to_string_lossy().to_string();
    if real != "Unknown" {
        name.set_text(&real);
    }
    card.append(&name);
    let email = gtk::Entry::new();
    email.set_placeholder_text(Some(match preset {
        Preset::Fastmail => "you@fastmail.com",
        Preset::Zoho => "you@zohomail.com",
    }));
    email.set_input_purpose(gtk::InputPurpose::Email);
    card.append(&email);
    let password = gtk::PasswordEntry::new();
    password.set_show_peek_icon(true);
    password.set_placeholder_text(Some("App password"));
    card.append(&password);

    let region = (preset == Preset::Zoho).then(|| {
        let options: Vec<(String, String)> = ZOHO_REGIONS.iter().map(|(d, l)| (d.to_string(), l.to_string())).collect();
        let dd = widgets::dropdown(&options, "zoho.com");
        card.append(&dd);
        dd
    });
    let region_of = {
        let region = region.clone();
        move || {
            region
                .as_ref()
                .and_then(|dd| ZOHO_REGIONS.get(dd.selected() as usize))
                .map(|(d, _)| d.to_string())
                .unwrap_or_else(|| "zoho.com".into())
        }
    };

    let link = gtk::Label::new(None);
    link.set_xalign(0.0);
    let set_link = {
        let (link, region_of) = (link.clone(), region_of.clone());
        move || {
            let url = preset.password_page(&region_of());
            link.set_markup(&format!("<a href=\"{}\">Make an app password</a>", glib::markup_escape_text(&url)));
        }
    };
    set_link();
    if let Some(dd) = &region {
        let set_link = set_link.clone();
        dd.connect_selected_notify(move |_| set_link());
    }
    card.append(&link);

    let dav = gtk::CheckButton::with_label("Also add calendar and contacts");
    dav.set_active(true);
    card.append(&dav);

    let error = widgets::label("", "dim");
    error.add_css_class("danger");
    error.set_wrap(true);
    error.set_visible(false);
    card.append(&error);

    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Cancel");
    let go = gtk::Button::with_label("Add");
    go.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&go);
    card.append(&buttons);
    dialog.set_child(Some(&card));

    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    let submit: Rc<dyn Fn()> = Rc::new({
        let (dialog, error, go) = (dialog.clone(), error.clone(), go.clone());
        let (name, email, password, dav) = (name.clone(), email.clone(), password.clone(), dav.clone());
        move || {
            let request = Request {
                preset,
                name: name.text().trim().to_string(),
                email: email.text().trim().to_string(),
                password: password.text().to_string(),
                region: region_of(),
                dav: dav.is_active(),
            };
            if let Err(msg) = check_form(&request) {
                error.set_text(&msg);
                error.set_visible(true);
                return;
            }
            error.set_visible(false);
            go.set_sensitive(false);
            go.set_label("Checking…");
            let (dialog, error, go) = (dialog.clone(), error.clone(), go.clone());
            cmd::background(
                move || save(&request),
                move |outcome: Outcome| match outcome {
                    Ok(note) => {
                        window::toast(note.as_deref().unwrap_or("Account saved."));
                        dialog.close();
                        super::online_accounts::show_list();
                    }
                    Err(msg) => {
                        error.set_text(&msg);
                        error.set_visible(true);
                        go.set_sensitive(true);
                        go.set_label("Add");
                    }
                },
            );
        }
    });
    let s = submit.clone();
    go.connect_clicked(move |_| s());
    let s = submit.clone();
    password.connect_activate(move |_| s());
    let keys = gtk::EventControllerKey::new();
    let close = dialog.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            close.close();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    dialog.add_controller(keys);
    dialog.present();
    email.grab_focus();
}

fn check_form(r: &Request) -> Result<(), String> {
    let (local, domain) = r.email.split_once('@').unwrap_or_default();
    if local.is_empty() || !domain.contains('.') || r.email.contains(char::is_whitespace) {
        return Err("Enter your full email address.".into());
    }
    if r.password.is_empty() {
        return Err("Enter the app password.".into());
    }
    if r.password.contains(['\r', '\n']) {
        return Err("That password has a line break in it.".into());
    }
    Ok(())
}

/// Check the password against the mail server, then save the account(s).
fn save(r: &Request) -> Outcome {
    let servers = r.preset.servers(&r.email, &r.region);
    let client = goa::Client::connect().map_err(|_| goa::Error::Unavailable.to_string())?;
    let existing = client.by_provider("imap_smtp").map_err(|e| e.to_string())?;
    if existing.iter().any(|a| a.identity.eq_ignore_ascii_case(&r.email)) {
        return Err("This account is already on this computer.".into());
    }
    let wrong = match r.preset {
        Preset::Fastmail => "The email address or app password is wrong.",
        Preset::Zoho => "The email address or app password is wrong. Check the region, and that IMAP access is on in Zoho Mail.",
    };
    imap_login(&servers.imap, &r.email, &r.password, wrong)?;

    let name = if r.name.is_empty() { r.email.clone() } else { r.name.clone() };
    let yes = || "true".to_string();
    let no = || "false".to_string();
    let mail_details: HashMap<String, String> = [
        ("MailEnabled", yes()),
        ("EmailAddress", r.email.clone()),
        ("Name", name),
        ("ImapHost", servers.imap.clone()),
        ("ImapUserName", r.email.clone()),
        ("ImapUseSsl", yes()),
        ("ImapUseTls", no()),
        ("ImapAcceptSslErrors", no()),
        ("SmtpHost", servers.smtp.clone()),
        ("SmtpUseAuth", yes()),
        ("SmtpAuthLogin", no()),
        ("SmtpAuthPlain", yes()),
        ("SmtpUserName", r.email.clone()),
        ("SmtpUseSsl", yes()),
        ("SmtpUseTls", no()),
        ("SmtpAcceptSslErrors", no()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    add_account(
        client.connection(),
        "imap_smtp",
        &r.email,
        &[("imap-password", &r.password), ("smtp-password", &r.password)],
        &mail_details,
    )?;

    if !r.dav {
        return Ok(None);
    }
    let dav_details: HashMap<String, String> = [
        ("Uri", servers.caldav.clone()),
        ("CalDavUri", servers.caldav.clone()),
        ("CardDavUri", servers.carddav.clone()),
        ("AcceptSslErrors", no()),
        ("CalendarEnabled", yes()),
        ("ContactsEnabled", yes()),
        ("FilesEnabled", no()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    match add_account(client.connection(), "webdav", &r.email, &[("password", &r.password)], &dav_details) {
        Ok(()) => Ok(None),
        Err(e) => Ok(Some(format!("Mail is set up. Calendar and contacts weren't added: {e}"))),
    }
}

/// `Manager.AddAccount`: GOA stores the details in its accounts file and the
/// credentials in the keyring.
fn add_account(
    conn: &gtk::gio::DBusConnection,
    provider: &str,
    identity: &str,
    credentials: &[(&str, &str)],
    details: &HashMap<String, String>,
) -> Result<(), String> {
    let creds = glib::VariantDict::new(None);
    for (k, v) in credentials {
        creds.insert_value(k, &v.to_variant());
    }
    let params = glib::Variant::tuple_from_iter([
        provider.to_variant(),
        identity.to_variant(),
        identity.to_variant(),
        creds.end(),
        details.to_variant(),
    ]);
    conn.call_sync(
        Some(goa::BUS_NAME),
        "/org/gnome/OnlineAccounts/Manager",
        "org.gnome.OnlineAccounts.Manager",
        "AddAccount",
        Some(&params),
        Some(glib::VariantTy::new("(o)").expect("reply type")),
        gtk::gio::DBusCallFlags::NONE,
        15000,
        gtk::gio::Cancellable::NONE,
    )
    .map(|_| ())
    .map_err(|e| goa::map_gio_error(&e).to_string())
}

/// Sign in to IMAP over TLS on 993 and log straight out.
fn imap_login(host: &str, user: &str, password: &str, wrong: &str) -> Result<(), String> {
    let unreachable = format!("Couldn't reach {host}. Check the network.");
    let socket = gtk::gio::SocketClient::new();
    socket.set_tls(true);
    socket.set_timeout(15);
    let conn = socket.connect_to_host(host, 993, gtk::gio::Cancellable::NONE).map_err(|_| unreachable.clone())?;
    let stream = conn.upcast::<gtk::gio::IOStream>();
    let mut reader = BufReader::new(stream.input_stream().into_read());
    let mut writer = stream.output_stream().into_write();
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|_| unreachable.clone())?;
    if !line.starts_with("* OK") {
        return Err(format!("{host} didn't answer like a mail server."));
    }
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    write!(writer, "a1 LOGIN {} {}\r\n", quote(user), quote(password)).map_err(|_| unreachable.clone())?;
    writer.flush().map_err(|_| unreachable.clone())?;
    loop {
        line.clear();
        if reader.read_line(&mut line).map_err(|_| unreachable.clone())? == 0 {
            return Err(format!("{host} closed the connection."));
        }
        if let Some(status) = line.strip_prefix("a1 ") {
            let _ = writer.write_all(b"a2 LOGOUT\r\n");
            let _ = writer.flush();
            return if status.starts_with("OK") { Ok(()) } else { Err(wrong.to_string()) };
        }
    }
}
