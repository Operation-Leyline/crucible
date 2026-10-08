static PATTERN_0: ::std::sync::LazyLock<::crucible_server_runtime_rust::__generated::regex::Regex> =
            ::std::sync::LazyLock::new(|| ::crucible_server_runtime_rust::__generated::regex::Regex::new("^[0-9]+$").expect("checked by crucible"));
static PATTERN_1: ::std::sync::LazyLock<::crucible_server_runtime_rust::__generated::regex::Regex> =
            ::std::sync::LazyLock::new(|| ::crucible_server_runtime_rust::__generated::regex::Regex::new("^[A-Za-z0-9_]+$").expect("checked by crucible"));
static PATTERN_2: ::std::sync::LazyLock<::crucible_server_runtime_rust::__generated::regex::Regex> =
            ::std::sync::LazyLock::new(|| ::crucible_server_runtime_rust::__generated::regex::Regex::new("^a[^\\n\\r\\x{2028}\\x{2029}]b$").expect("checked by crucible"));
static PATTERN_3: ::std::sync::LazyLock<::crucible_server_runtime_rust::__generated::regex::Regex> =
            ::std::sync::LazyLock::new(|| ::crucible_server_runtime_rust::__generated::regex::Regex::new("^[\\t-\\r \\x{00A0}\\x{1680}\\x{2000}-\\x{200A}\\x{2028}\\x{2029}\\x{202F}\\x{205F}\\x{3000}\\x{FEFF}]$").expect("checked by crucible"));
static PATTERN_4: ::std::sync::LazyLock<::crucible_server_runtime_rust::__generated::regex::Regex> =
            ::std::sync::LazyLock::new(|| ::crucible_server_runtime_rust::__generated::regex::Regex::new("(?-u:\\b)a(?-u:\\b)").expect("checked by crucible"));
static PATTERN_5: ::std::sync::LazyLock<::crucible_server_runtime_rust::__generated::regex::Regex> =
            ::std::sync::LazyLock::new(|| ::crucible_server_runtime_rust::__generated::regex::Regex::new("^[[^0-9][^A-Za-z0-9_][^\\t-\\r \\x{00A0}\\x{1680}\\x{2000}-\\x{200A}\\x{2028}\\x{2029}\\x{202F}\\x{205F}\\x{3000}\\x{FEFF}]\\x08.]+$").expect("checked by crucible"));
static PATTERN_6: ::std::sync::LazyLock<::crucible_server_runtime_rust::__generated::regex::Regex> =
            ::std::sync::LazyLock::new(|| ::crucible_server_runtime_rust::__generated::regex::Regex::new("^\\\\d\\.\\x{00e9}\\x{41}\\x{01}$").expect("checked by crucible"));
static PATTERN_7: ::std::sync::LazyLock<::crucible_server_runtime_rust::__generated::regex::Regex> =
            ::std::sync::LazyLock::new(|| ::crucible_server_runtime_rust::__generated::regex::Regex::new("^[a\\&\\&b\\~\\~c]$").expect("checked by crucible"));
static PATTERN_8: ::std::sync::LazyLock<::crucible_server_runtime_rust::__generated::regex::Regex> =
            ::std::sync::LazyLock::new(|| ::crucible_server_runtime_rust::__generated::regex::Regex::new("^[^\\x{0}-\\x{10FFFF}]$").expect("checked by crucible"));
static PATTERN_9: ::std::sync::LazyLock<::crucible_server_runtime_rust::__generated::regex::Regex> =
            ::std::sync::LazyLock::new(|| ::crucible_server_runtime_rust::__generated::regex::Regex::new("^[\\x{0}-\\x{10FFFF}]$").expect("checked by crucible"));
static PATTERN_10: ::std::sync::LazyLock<::crucible_server_runtime_rust::__generated::regex::Regex> =
            ::std::sync::LazyLock::new(|| ::crucible_server_runtime_rust::__generated::regex::Regex::new("^[\\x{0}-\\x{10FFFF}]x\\]$").expect("checked by crucible"));
