fn main() {
    println!("cargo:rerun-if-changed=src/helper.c");
    match pkg_config::Config::new().probe("goa-backend-1.0") {
        Ok(lib) => {
            let mut b = cc::Build::new();
            b.file("src/helper.c");
            b.define("GOA_BACKEND_API_IS_SUBJECT_TO_CHANGE", None);
            b.define("GOA_API_IS_SUBJECT_TO_CHANGE", None);
            for p in &lib.include_paths {
                b.include(p);
            }
            b.compile("nixe_goa_native");
            println!("cargo:rustc-cfg=has_goa_backend");
        }
        Err(e) => {
            println!("cargo:warning=goa-backend-1.0 missing ({e}); nixe-goa-helper is a stub");
        }
    }
}
