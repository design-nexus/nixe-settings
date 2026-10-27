//! User-session helper. argv is list / add TYPE / refresh ID — no secrets.

use std::env;
use std::ffi::CString;
use std::os::raw::{c_char, c_int};

#[cfg(has_goa_backend)]
unsafe extern "C" {
    fn nyx_goa_run(argc: c_int, argv: *const *const c_char) -> c_int;
}

fn main() {
    #[cfg(not(has_goa_backend))]
    {
        eprintln!("Install gnome-online-accounts to add accounts from Settings.");
        std::process::exit(1);
    }
    #[cfg(has_goa_backend)]
    {
        let args: Vec<CString> = env::args().map(|a| CString::new(a).expect("arg")).collect();
        let ptrs: Vec<*const c_char> = args.iter().map(|c| c.as_ptr()).collect();
        let rc = unsafe { nyx_goa_run(ptrs.len() as c_int, ptrs.as_ptr()) };
        std::process::exit(rc);
    }
}
