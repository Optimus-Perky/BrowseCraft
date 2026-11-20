//! BrowseCraft sync server: Chrome-style sync (bookmarks, passwords, history, open tabs,
//! extensions, settings) for BrowseCraft browsers, on your own server.
//!
//!   browsecraft-server serve                    run it (BROWSECRAFT_LISTEN, BROWSECRAFT_DB)
//!   browsecraft-server add-user NAME [--admin]  password from BROWSECRAFT_PASSWORD or stdin
//!   browsecraft-server set-password NAME
//!   browsecraft-server delete-user NAME
//!   browsecraft-server users
//!
//! On first start with no users, BROWSECRAFT_ADMIN_USER and BROWSECRAFT_ADMIN_PASSWORD make the
//! first (admin) user. Domain sign-in (Active Directory / Samba AD): see ldap.rs.

mod admin;
mod crx;
mod extstore;
mod kerberos;
mod ldap;
mod proto;
mod store;
mod storeweb;
mod sync;
mod web;

use std::io::BufRead;

fn env(name: &str, default: &str) -> String {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| default.to_string())
}

fn read_password() -> String {
    if let Ok(p) = std::env::var("BROWSECRAFT_PASSWORD") {
        return p;
    }
    eprint!("Password: ");
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    line.trim_end_matches(['\r', '\n']).to_string()
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let db_path = env("BROWSECRAFT_DB", "browsecraft.db");
    let db = store::open(&db_path).map_err(|e| format!("could not open {db_path}: {e}"))?;
    match args.first().map(String::as_str).unwrap_or("serve") {
        "serve" => {
            if store::user_count(&db).map_err(|e| e.to_string())? == 0 {
                match (std::env::var("BROWSECRAFT_ADMIN_USER"), std::env::var("BROWSECRAFT_ADMIN_PASSWORD")) {
                    (Ok(u), Ok(p)) => {
                        store::add_user(&db, &u, &p, true)?;
                        eprintln!("Made the first user, {u} (admin).");
                    }
                    _ => eprintln!("No users yet: run `altbrowse-server add-user NAME --admin`, or open the web page to make the first one."),
                }
            }
            let listen = env("BROWSECRAFT_LISTEN", "0.0.0.0:8295");
            let ldap = ldap::Config::from_env()?;
            let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|e| e.to_string())?;
            rt.block_on(web::serve(db, &listen, ldap))
        }
        "add-user" => {
            let name = args.get(1).ok_or("add-user NAME [--admin]")?;
            let admin = args.iter().any(|a| a == "--admin");
            let u = store::add_user(&db, name, &read_password(), admin)?;
            println!("Added {}{}.", u.name, if u.is_admin { " (admin)" } else { "" });
            Ok(())
        }
        "set-password" => {
            let name = args.get(1).ok_or("set-password NAME")?;
            store::set_password(&db, name, &read_password())?;
            println!("Password changed for {name}.");
            Ok(())
        }
        "delete-user" => {
            let name = args.get(1).ok_or("delete-user NAME")?;
            match store::delete_user(&db, name).map_err(|e| e.to_string())? {
                0 => Err(format!("no user called {name}")),
                _ => {
                    println!("Deleted {name} and everything synced for them.");
                    Ok(())
                }
            }
        }
        "users" => {
            for (u, devices, items, source) in store::users(&db).map_err(|e| e.to_string())? {
                println!("{}{}{}  {devices} devices, {items} items", u.name, if u.is_admin { " (admin)" } else { "" }, if source == "domain" { " (domain)" } else { "" });
            }
            Ok(())
        }
        "--version" | "version" => {
            println!("altbrowse-server {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => Err(format!("unknown command {other}; try serve, add-user, set-password, delete-user, users")),
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("altbrowse-server: {e}");
        std::process::exit(1);
    }
}
