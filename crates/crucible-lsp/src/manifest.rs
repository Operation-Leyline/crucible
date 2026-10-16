//! `crucible.toml` in the editor: completion and hover for every table, key
//! and value the compiler (`[model]`) and `crucible-policy` read. The parsers
//! stay the authority; a test writes a manifest from this vocabulary and
//! checks both accept it, so the two cannot drift apart unnoticed.

use crucible_policy::NameCase;
use lsp_types::{CompletionItem, CompletionItemKind, Documentation, MarkupContent, MarkupKind};

/// What a key takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Free text: a namespace, a path, a pattern.
    Text,
    /// Strings, free text.
    Texts,
    /// The language version.
    Version,
    /// `error`, `warning`, `off`.
    Severity,
    /// A bundled package to import.
    Imports,
    /// A naming case, bare or `{ case = .., severity = .. }`.
    Case,
    /// A whole-name regex, bare or `{ pattern = .., severity = .. }`.
    Pattern,
    /// An absolute path prefix, bare or `{ path = .., severity = .. }`.
    Prefix,
    /// `true`/`false`, bare or `{ enabled = .., severity = .. }`.
    Enabled,
    /// Documentation targets, each bare or `{ target = .., severity = .. }`.
    Targets,
    /// A policy rule id.
    Rule,
}

impl Kind {
    /// The field naming the value in the check's table form.
    fn check_key(self) -> Option<&'static str> {
        Some(match self {
            Kind::Case => "case",
            Kind::Pattern => "pattern",
            Kind::Prefix => "path",
            Kind::Enabled => "enabled",
            Kind::Targets => "target",
            _ => return None,
        })
    }
}

struct Key {
    name: &'static str,
    kind: Kind,
    doc: &'static str,
    /// A value that parses, for the vocabulary test.
    #[cfg_attr(not(test), allow(dead_code))]
    sample: &'static str,
}

struct Table {
    /// The header path: `model`, `rules.naming`; `""` is the top level.
    name: &'static str,
    /// `[[exceptions]]`: an array of tables.
    array: bool,
    doc: &'static str,
    keys: &'static [Key],
}

const SEVERITY: Key = Key {
    name: "severity",
    kind: Kind::Severity,
    doc: "The default severity of this group's checks: `error` fails the build, `warning` is reported, `off` skips them. A check written as a table can set its own.",
    sample: "\"warning\"",
};

const TABLES: &[Table] = &[
    Table {
        name: "",
        array: false,
        doc: "",
        keys: &[Key {
            name: "schema_version",
            kind: Kind::Version,
            doc: "The policy format version. Optional; 1 is the only version.",
            sample: "1",
        }],
    },
    Table {
        name: "model",
        array: false,
        doc: "Makes this directory a Crucible language package: the namespace and where its `.cru` files are.",
        keys: &[
            Key {
                name: "namespace",
                kind: Kind::Text,
                doc: "The package's namespace, such as `example.service`. Required.",
                sample: "\"example.shop\"",
            },
            Key {
                name: "language",
                kind: Kind::Version,
                doc: "The language version the package is written in; 1 is the only version.",
                sample: "1",
            },
            Key {
                name: "sources",
                kind: Kind::Texts,
                doc: "Directories, relative to this file, whose `.cru` files belong to the package. Defaults to `[\".\"]`.",
                sample: "[\".\"]",
            },
            Key {
                name: "imports",
                kind: Kind::Imports,
                doc: "Bundled packages whose declarations this package uses, such as `crucible.errors`.",
                sample: "[\"crucible.errors\"]",
            },
        ],
    },
    Table {
        name: "model.naming",
        array: false,
        doc: "How the anonymous inputs and outputs of operations are named.",
        keys: &[
            Key {
                name: "input",
                kind: Kind::Text,
                doc: "The input structure's name; must contain `{Operation}`. Defaults to `{Operation}Input`.",
                sample: "\"{Operation}Request\"",
            },
            Key {
                name: "output",
                kind: Kind::Text,
                doc: "The output structure's name; must contain `{Operation}`. Defaults to `{Operation}Output`.",
                sample: "\"{Operation}Response\"",
            },
        ],
    },
    Table {
        name: "model.baseline",
        array: false,
        doc: "The last release, frozen as a snapshot of its compiled model. Every build and the editor compare the package against it, and a breaking change is an error. Make one with `crucible freeze <package>`, or `crucible freeze <package> --from <git ref>` for a past release.",
        keys: &[Key {
            name: "path",
            kind: Kind::Text,
            doc: "The release's snapshot, relative to this file, such as `releases/2026-09-04.json`.",
            sample: "\"releases/v1.json\"",
        }],
    },
    Table {
        name: "rules",
        array: false,
        doc: "API policy checked on every build and in the editor.",
        keys: &[Key {
            name: "namespaces",
            kind: Kind::Texts,
            doc: "The namespaces the static rules check. Empty checks the service's own namespace.",
            sample: "[\"example.shop\"]",
        }],
    },
    Table {
        name: "rules.naming",
        array: false,
        doc: "Naming conventions. Each check is a case (`operations = \"PascalCase\"`) or, to set its own severity, a table (`members = { case = \"camelCase\", severity = \"warning\" }`).",
        keys: &[
            SEVERITY,
            Key {
                name: "operations",
                kind: Kind::Case,
                doc: "The case of operation names.",
                sample: "\"PascalCase\"",
            },
            Key {
                name: "shapes",
                kind: Kind::Case,
                doc: "The case of shape names: structures, unions, enums, errors and named scalars.",
                sample: "{ case = \"PascalCase\", severity = \"error\" }",
            },
            Key {
                name: "members",
                kind: Kind::Case,
                doc: "The case of member names.",
                sample: "{ case = \"camelCase\", severity = \"warning\" }",
            },
            Key {
                name: "enum_values",
                kind: Kind::Case,
                doc: "The case of enum value names.",
                sample: "\"SCREAMING_SNAKE_CASE\"",
            },
            Key {
                name: "operations_pattern",
                kind: Kind::Pattern,
                doc: "A regex every operation name must match whole, such as `(Get|List|Create|Update|Delete)[A-Z].*`.",
                sample: "\"[A-Z].*\"",
            },
            Key {
                name: "shapes_pattern",
                kind: Kind::Pattern,
                doc: "A regex every shape name must match whole.",
                sample: "{ pattern = \"[A-Z].*\", severity = \"warning\" }",
            },
            Key {
                name: "members_pattern",
                kind: Kind::Pattern,
                doc: "A regex every member name must match whole.",
                sample: "\"[a-z].*\"",
            },
            Key {
                name: "enum_values_pattern",
                kind: Kind::Pattern,
                doc: "A regex every enum value name must match whole.",
                sample: "\"[A-Z_]+\"",
            },
        ],
    },
    Table {
        name: "rules.paths",
        array: false,
        doc: "URI conventions for HTTP operations.",
        keys: &[
            SEVERITY,
            Key {
                name: "prefix",
                kind: Kind::Prefix,
                doc: "A prefix every operation's path starts with, at a segment boundary, such as `/v1`.",
                sample: "\"/v1\"",
            },
            Key {
                name: "segment_case",
                kind: Kind::Case,
                doc: "The case of literal path segments.",
                sample: "\"kebab-case\"",
            },
            Key {
                name: "parameter_case",
                kind: Kind::Case,
                doc: "The case of path parameters (`{label}`).",
                sample: "{ case = \"camelCase\", severity = \"warning\" }",
            },
        ],
    },
    Table {
        name: "rules.documentation",
        array: false,
        doc: "What must be documented with `///`.",
        keys: &[
            SEVERITY,
            Key {
                name: "targets",
                kind: Kind::Targets,
                doc: "What must be documented: `service`, `operations`, `shapes`, `members`, `errors`, `enums`. An entry may be `{ target = \"errors\", severity = \"warning\" }`.",
                sample: "[\"operations\", { target = \"errors\", severity = \"warning\" }]",
            },
        ],
    },
    Table {
        name: "rules.examples",
        array: false,
        doc: "What the API reference needs beyond the types: examples, and documented errors.",
        keys: &[
            Key {
                name: "severity",
                kind: Kind::Severity,
                doc: "The default severity of this group's checks, `warning` unless set: `error` fails the build, `off` skips them. A check written as a table can set its own.",
                sample: "\"error\"",
            },
            Key {
                name: "operations",
                kind: Kind::Enabled,
                doc: "Each operation declares at least one `example`; without one, the reference shows a generated sample.",
                sample: "true",
            },
            Key {
                name: "errors",
                kind: Kind::Enabled,
                doc: "Each error an operation declares appears in one of its examples (`error Name { .. }`); the service's shared errors are not asked of each operation.",
                sample: "{ enabled = true, severity = \"error\" }",
            },
        ],
    },
    Table {
        name: "compatibility",
        array: false,
        doc: "What `crucible diff` protects when comparing a contract with its last release.",
        keys: &[
            SEVERITY,
            Key {
                name: "wire",
                kind: Kind::Enabled,
                doc: "Check that old clients keep working against the new service.",
                sample: "true",
            },
            Key {
                name: "sdk_source",
                kind: Kind::Enabled,
                doc: "Check that code using generated SDKs keeps compiling.",
                sample: "{ enabled = true, severity = \"warning\" }",
            },
        ],
    },
    Table {
        name: "exceptions",
        array: true,
        doc: "An accepted violation: the rule, the shape and why.",
        keys: &[
            Key {
                name: "rule",
                kind: Kind::Rule,
                doc: "The rule the exception is for, such as `naming.members`.",
                sample: "\"naming.members\"",
            },
            Key {
                name: "shape",
                kind: Kind::Text,
                doc: "The shape, `namespace#Shape`, or member, `namespace#Shape$member`.",
                sample: "\"example.shop#Item$legacy_id\"",
            },
            Key {
                name: "reason",
                kind: Kind::Text,
                doc: "Why the violation is accepted. Required.",
                sample: "\"wire name from v1\"",
            },
        ],
    },
];

const TARGETS: &[&str] = &[
    "service",
    "operations",
    "shapes",
    "members",
    "errors",
    "enums",
];

/// The table the line at `line_start` is in: the last header above it.
fn table_at(text: &str, line_start: usize) -> &'static str {
    let mut current = "";
    for line in text[..line_start].lines() {
        let trimmed = line.trim();
        if let Some(header) = trimmed.strip_prefix('[') {
            let name: String = header
                .trim_start_matches('[')
                .chars()
                .take_while(|c| *c != ']')
                .filter(|c| !c.is_whitespace())
                .collect();
            current = TABLES
                .iter()
                .find(|table| table.name == name)
                .map_or("?", |table| table.name);
        }
    }
    current
}

fn table(name: &str) -> Option<&'static Table> {
    TABLES.iter().find(|table| table.name == name)
}

fn markdown(text: &str) -> Documentation {
    Documentation::MarkupContent(MarkupContent {
        kind: MarkupKind::Markdown,
        value: text.to_owned(),
    })
}

fn item(label: &str, kind: CompletionItemKind, doc: &str, insert: String) -> CompletionItem {
    CompletionItem {
        label: label.to_owned(),
        kind: Some(kind),
        documentation: (!doc.is_empty()).then(|| markdown(doc)),
        insert_text: Some(insert),
        ..Default::default()
    }
}

/// Completion at `offset` in a `crucible.toml`.
pub fn complete(text: &str, offset: usize) -> Vec<CompletionItem> {
    let line_start = text[..offset].rfind('\n').map_or(0, |at| at + 1);
    let before = &text[line_start..offset];
    let trimmed = before.trim_start();

    // A header: `[mo|` or `[[ex|`.
    if let Some(rest) = trimmed.strip_prefix('[') {
        let array = rest.starts_with('[');
        return TABLES
            .iter()
            .filter(|table| !table.name.is_empty() && table.array == array)
            .map(|table| {
                let close = if table.array { "]]" } else { "]" };
                item(
                    table.name,
                    CompletionItemKind::MODULE,
                    table.doc,
                    format!("{}{close}", table.name),
                )
            })
            .collect();
    }

    let current = table_at(text, line_start);
    let Some(table) = table(current) else {
        return Vec::new();
    };
    let Some((key_text, value)) = trimmed.split_once('=') else {
        // A key: the table's keys, and at the top level the tables too.
        let mut out: Vec<CompletionItem> = table
            .keys
            .iter()
            .map(|key| {
                item(
                    key.name,
                    CompletionItemKind::PROPERTY,
                    key.doc,
                    format!("{} = ", key.name),
                )
            })
            .collect();
        if table.name.is_empty() {
            out.extend(TABLES.iter().filter(|t| !t.name.is_empty()).map(|t| {
                let (open, close) = if t.array { ("[[", "]]") } else { ("[", "]") };
                item(
                    &format!("{open}{}{close}", t.name),
                    CompletionItemKind::MODULE,
                    t.doc,
                    format!("{open}{}{close}\n", t.name),
                )
            }));
        }
        return out;
    };
    let Some(key) = table.keys.iter().find(|key| key.name == key_text.trim()) else {
        return Vec::new();
    };

    // Inside an inline table, `{ case = .., severity = .. }`: the open
    // brace's contents decide.
    let inline = value
        .rfind('{')
        .filter(|open| !value[*open..].contains('}'))
        .map(|open| &value[open + 1..]);
    let quoted = before.ends_with(['"', '\'']);
    let string = |value: &str| {
        if quoted {
            value.to_owned()
        } else {
            format!("\"{value}\"")
        }
    };
    let values = |kind: Kind| -> Vec<CompletionItem> {
        let enum_value = |label: &str, doc: &str| {
            item(label, CompletionItemKind::ENUM_MEMBER, doc, string(label))
        };
        match kind {
            Kind::Severity => vec![
                enum_value("error", "Fails the build."),
                enum_value("warning", "Reported; the build succeeds."),
                enum_value("off", "Not checked."),
            ],
            Kind::Case => NameCase::ALL
                .iter()
                .map(|case| {
                    enum_value(
                        case.name(),
                        &format!("For example `{}`.", case.apply("listItems")),
                    )
                })
                .collect(),
            Kind::Targets => TARGETS
                .iter()
                .map(|target| enum_value(target, ""))
                .collect(),
            Kind::Rule => crucible_policy::RULE_IDS
                .iter()
                .map(|rule| enum_value(rule, ""))
                .collect(),
            Kind::Imports => crucible_lang::bundled_packages()
                .map(|package| enum_value(package, "A package bundled with the compiler."))
                .collect(),
            Kind::Enabled => ["true", "false"]
                .iter()
                .map(|value| item(value, CompletionItemKind::VALUE, "", (*value).to_owned()))
                .collect(),
            Kind::Version => vec![item("1", CompletionItemKind::VALUE, "", "1".to_owned())],
            _ => Vec::new(),
        }
    };
    if let Some(inline) = inline {
        let field = inline.rsplit(',').next().unwrap_or(inline);
        return match field.split_once('=') {
            // A field: the check's value field and `severity`.
            None => key
                .kind
                .check_key()
                .into_iter()
                .chain(["severity"])
                .map(|field| {
                    item(
                        field,
                        CompletionItemKind::PROPERTY,
                        if field == "severity" {
                            "This check's own severity."
                        } else {
                            key.doc
                        },
                        format!("{field} = "),
                    )
                })
                .collect(),
            Some((field, _)) if field.trim() == "severity" => values(Kind::Severity),
            Some(_) => values(key.kind),
        };
    }
    values(key.kind)
}

/// Hover text for the key or table header under `offset`.
pub fn hover(text: &str, offset: usize) -> Option<String> {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.';
    let start = text[..offset]
        .rfind(|c: char| !is_word(c))
        .map_or(0, |at| at + 1);
    let end = text[offset..]
        .find(|c: char| !is_word(c))
        .map_or(text.len(), |at| offset + at);
    let word = &text[start..end];
    if word.is_empty() {
        return None;
    }
    if text[..start].ends_with('[') {
        let table = table(word)?;
        return Some(format!("**[{}]**\n\n{}", table.name, table.doc));
    }
    let line_start = text[..start].rfind('\n').map_or(0, |at| at + 1);
    let key = table(table_at(text, line_start))?
        .keys
        .iter()
        .find(|key| key.name == word)?;
    Some(format!("**{}**\n\n{}", key.name, key.doc))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(text: &str) -> Vec<String> {
        let offset = text.find('|').unwrap();
        let text = text.replace('|', "");
        complete(&text, offset)
            .into_iter()
            .map(|item| item.label)
            .collect()
    }

    #[test]
    fn completion_follows_the_table_key_and_value() {
        assert!(labels("[|").contains(&"rules.naming".to_owned()));
        assert_eq!(labels("[[|"), ["exceptions"]);
        let keys = labels("[rules.naming]\n|");
        assert!(keys.contains(&"members".to_owned()) && keys.contains(&"severity".to_owned()));
        assert!(labels("[rules.naming]\nmembers = |").contains(&"camelCase".to_owned()));
        assert!(labels("[rules.naming]\nmembers = \"|").contains(&"PascalCase".to_owned()));
        assert_eq!(
            labels("[rules.naming]\nmembers = { |"),
            ["case", "severity"]
        );
        assert!(
            labels("[rules.naming]\nmembers = { case = \"camelCase\", severity = |")
                .contains(&"warning".to_owned())
        );
        assert!(labels("[rules.naming]\nmembers = { case = |").contains(&"snake_case".to_owned()));
        assert!(
            labels("[rules.documentation]\ntargets = [\"errors\", |")
                .contains(&"operations".to_owned())
        );
        assert!(labels("[[exceptions]]\nrule = |").contains(&"naming.members".to_owned()));
        assert_eq!(labels("[model]\nimports = [|"), ["crucible.errors"]);
        let top = labels("|");
        assert!(top.contains(&"[model]".to_owned()) && top.contains(&"schema_version".to_owned()));
        // A string value is quoted unless the quote is already typed.
        let offset = "[rules.naming]\nmembers = ".len();
        let items = complete("[rules.naming]\nmembers = ", offset);
        assert_eq!(items[0].insert_text.as_deref(), Some("\"PascalCase\""));
    }

    #[test]
    fn hover_explains_keys_and_tables() {
        let text = "[rules.naming]\nmembers = \"camelCase\"\n";
        assert!(
            hover(text, text.find("members").unwrap() + 2)
                .unwrap()
                .contains("member names")
        );
        assert!(hover(text, 3).unwrap().contains("Naming conventions"));
    }

    /// Every key and sample value in the vocabulary is one the parsers accept.
    #[test]
    fn the_vocabulary_matches_the_parsers() {
        let mut manifest = String::new();
        for table in TABLES {
            if !table.name.is_empty() {
                let (open, close) = if table.array {
                    ("[[", "]]")
                } else {
                    ("[", "]")
                };
                manifest.push_str(&format!("\n{open}{}{close}\n", table.name));
            }
            for key in table.keys {
                manifest.push_str(&format!("{} = {}\n", key.name, key.sample));
            }
        }
        // The sample baseline is a frozen release; the sources are `.`.
        let dir = std::env::temp_dir().join(format!("crucible-vocabulary-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("releases")).unwrap();
        std::fs::write(dir.join("releases/v1.json"), "{}").unwrap();
        let problems = crucible_lang::check_manifest(&dir, &manifest);
        assert_eq!(problems, Vec::new(), "{manifest}");
        if let Err(error) = crucible_policy::Config::parse_located(&manifest) {
            panic!("{error}\n{manifest}");
        }
    }
}
