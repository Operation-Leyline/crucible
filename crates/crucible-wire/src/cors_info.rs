//! Static service CORS policy shared with generated server bindings.
#[derive(Debug)]
pub struct CorsInfo {
    pub origin: &'static str,
    pub max_age: i32,
    pub allow_credentials: bool,
    pub allowed_headers: &'static [&'static str],
    pub allowed_header_prefixes: &'static [&'static str],
    pub exposed_headers: &'static [&'static str],
    pub methods: &'static [&'static str],
}
