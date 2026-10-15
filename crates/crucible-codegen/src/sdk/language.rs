/// A client language, spelled as `crucible generate` spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Language {
    Rust,
    Ts,
    Python,
    Go,
    Java,
    Kotlin,
    Csharp,
    Swift,
    Dart,
    Php,
    Ruby,
    Cpp,
    C,
    R,
}

/// Every language key the file knows. Which a language takes is
/// [`Language::keys`]; one it does not take is an error at that key.
pub(crate) const LANGUAGE_KEYS: &[&str] = &[
    "package",
    "group_id",
    "artifact_id",
    "package_id",
    "module_name",
    "namespace",
    "prefix",
    "module_path",
    "client_name",
    "endpoint",
    "middleware",
    "package_json",
    "runtime",
    "runtime_path",
];

impl Language {
    pub const ALL: [Self; 14] = [
        Self::Rust,
        Self::Ts,
        Self::Python,
        Self::Go,
        Self::Java,
        Self::Kotlin,
        Self::Csharp,
        Self::Swift,
        Self::Dart,
        Self::Php,
        Self::Ruby,
        Self::Cpp,
        Self::C,
        Self::R,
    ];

    /// The `crucible generate` spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Ts => "ts",
            Self::Python => "python",
            Self::Go => "go",
            Self::Java => "java",
            Self::Kotlin => "kotlin",
            Self::Csharp => "csharp",
            Self::Swift => "swift",
            Self::Dart => "dart",
            Self::Php => "php",
            Self::Ruby => "ruby",
            Self::Cpp => "cpp",
            Self::C => "c",
            Self::R => "r",
        }
    }

    /// How people write the language's name in prose.
    pub fn display(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Ts => "TypeScript",
            Self::Python => "Python",
            Self::Go => "Go",
            Self::Java => "Java",
            Self::Kotlin => "Kotlin",
            Self::Csharp => "C#",
            Self::Swift => "Swift",
            Self::Dart => "Dart",
            Self::Php => "PHP",
            Self::Ruby => "Ruby",
            Self::Cpp => "C++",
            Self::C => "C",
            Self::R => "R",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.name() == name)
    }

    /// The language keys this language's generator takes.
    pub(crate) fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["package", "runtime", "runtime_path"],
            Self::Ts => &[
                "package",
                "client_name",
                "endpoint",
                "middleware",
                "package_json",
            ],
            Self::Python | Self::Swift | Self::Ruby => &["package", "module_name", "endpoint"],
            Self::Go => &["package", "module_path", "endpoint"],
            Self::Java | Self::Kotlin => &["package", "group_id", "artifact_id", "endpoint"],
            Self::Csharp => &["package_id", "namespace", "endpoint"],
            Self::Php | Self::Cpp => &["package", "namespace", "endpoint"],
            Self::C => &["package", "prefix", "endpoint"],
            Self::Dart | Self::R => &["package", "endpoint"],
        }
    }

    /// The keys `crucible generate <language>` requires, which the file
    /// must set too: a release never takes a placeholder coordinate.
    pub(crate) fn required(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &[],
            Self::Ts | Self::Dart | Self::R => &["package"],
            Self::Python | Self::Swift | Self::Ruby => &["package", "module_name"],
            Self::Go => &["package", "module_path"],
            Self::Java | Self::Kotlin => &["package", "group_id", "artifact_id"],
            Self::Csharp => &["package_id", "namespace"],
            Self::Php | Self::Cpp => &["package", "namespace"],
            Self::C => &["package", "prefix"],
        }
    }

    /// The version `crucible sdk generate` writes development exports at.
    pub fn development_version(self) -> Option<String> {
        match self {
            Self::Ts | Self::Dart | Self::Swift | Self::Csharp => Some("0.0.0-dev".into()),
            Self::Python => Some("0.0.0.dev0".into()),
            Self::Java | Self::Kotlin => Some("0.0.0-SNAPSHOT".into()),
            Self::Ruby | Self::Php | Self::C | Self::Cpp | Self::R => Some("0.0.0".into()),
            Self::Rust => Some(env!("CARGO_PKG_VERSION").into()),
            Self::Go => None,
        }
    }
}
