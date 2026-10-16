//! A package imports another by path (`[model.dependencies]`, decision 60),
//! and the rules of what it imports apply to it and cannot be weakened
//! (decision 47).

use std::path::{Path, PathBuf};

use crucible_lang::{Compiled, check_manifest, compile, load_package};

fn write(dir: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

/// A scratch directory for one test, empty.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("deps-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const STANDARDS: &str = r#"
metadata owned on operation

/// Every operation is owned.
rule Owned strict {
    every operation
    must has @owned
}

/// A string has a length.
rule Bounded {
    every string where not enum
    must has length
}

/// A document accepts any JSON.
rule NoDocument exceptable {
    no document
}

/// Advice: write a doc.
rule Documented advice {
    every operation must documented
}

except Bounded for Unbounded because "the standards package excuses its own shape"

string Unbounded
"#;

fn standards(dir: &Path) {
    write(
        dir,
        &[
            (
                "standards/crucible.toml",
                "[model]\nnamespace = \"org.standards\"\nsources = [\"policy\"]\n",
            ),
            ("standards/policy/standards.cru", STANDARDS),
        ],
    );
}

const SHOP: &str = r#"
service Shop {
    version "1"
    protocol json
    operations [Ping, Hold]
}

string Name { length 1..10 }
string Loose
document Open

@owned
readonly operation Ping {
    http GET "/ping"
    output { name: Name }
}

@owned
readonly operation Hold {
    http GET "/hold"
    output { loose: Loose, open: Open }
}
"#;

fn shop(dir: &Path, imports: &str, policy: &str) {
    write(
        dir,
        &[
            (
                "shop/crucible.toml",
                &format!(
                    "[model]\nnamespace = \"example.shop\"\nsources = [\"model\", \"policy\"]\nimports = [{imports}]\n\n[model.dependencies]\n\"org.standards\" = {{ path = \"../standards\" }}\n"
                ),
            ),
            ("shop/model/shop.cru", SHOP),
            ("shop/policy/shop.cru", policy),
        ],
    );
}

fn compiled(dir: &Path) -> Compiled {
    compile(&load_package(&dir.join("shop")).unwrap(), None)
}

fn errors(compiled: &Compiled) -> Vec<String> {
    compiled
        .diagnostics
        .iter()
        .filter(|d| d.diagnostic.is_error())
        .map(|d| d.diagnostic.message.clone())
        .collect()
}

fn mentions(found: &[String], needle: &str) {
    assert!(
        found.iter().any(|m| m.contains(needle)),
        "nothing mentions {needle:?}: {found:#?}"
    );
}

#[test]
fn an_imported_packages_rules_apply_to_the_importer() {
    let dir = scratch("apply");
    standards(&dir);
    shop(&dir, "\"org.standards\"", "");
    let compiled = compiled(&dir);
    let found = errors(&compiled);
    // `Loose` has no length and `Open` is a document; `Ping` and `Hold` are owned.
    mentions(&found, "rule org.standards#Bounded: `Loose`");
    mentions(&found, "rule org.standards#NoDocument: `Open`");
    assert!(
        found.iter().all(|m| !m.contains("Unbounded")),
        "the standards' own shapes are judged when they compile: {found:#?}"
    );
    assert_eq!(found.len(), 2, "{found:#?}");
    // The advice does not fail anything.
    assert!(compiled.diagnostics.iter().any(|d| {
        d.diagnostic
            .message
            .starts_with("rule org.standards#Documented")
    }));
}

#[test]
fn an_import_brings_the_packages_metadata_into_scope() {
    let dir = scratch("scope");
    standards(&dir);
    shop(&dir, "\"org.standards\"", "");
    // `@owned` on the operations resolved, so no unknown-annotation error.
    assert!(
        errors(&compiled(&dir))
            .iter()
            .all(|m| !m.contains("unknown annotation")),
    );
}

#[test]
fn only_an_exceptable_rule_can_be_excepted_by_an_importer() {
    let dir = scratch("except");
    standards(&dir);
    shop(
        &dir,
        "\"org.standards\"",
        r#"
except NoDocument for Open because "Open JSON the service bounds."
except Bounded for Loose because "Not allowed: Bounded belongs to the standards."
except Owned for Ping because "Strict."
"#,
    );
    let compiled = compiled(&dir);
    let found = errors(&compiled);
    mentions(
        &found,
        "rule `Bounded` belongs to `org.standards` and is not `exceptable`",
    );
    mentions(&found, "rule `Owned` is strict");
    // The excepted document no longer fails.
    assert!(
        found
            .iter()
            .all(|m| !m.contains("rule org.standards#NoDocument")),
        "{found:#?}"
    );
    // The violation the importer may not except stands.
    mentions(&found, "rule org.standards#Bounded: `Loose`");
}

#[test]
fn an_exception_of_an_imported_rule_is_recorded_with_both_packages() {
    let dir = scratch("recorded");
    standards(&dir);
    shop(
        &dir,
        "\"org.standards\"",
        r#"
string Tight { length 1..2 }
except NoDocument for Open because "Open JSON the service bounds."
"#,
    );
    // `Loose` still fails `Bounded`, so the package does not compile; the
    // exception is recorded on the best-effort model all the same.
    let compiled = compiled(&dir);
    let model = compiled.best_effort.as_ref().expect("best effort");
    let exception = &model.exceptions[0];
    assert_eq!(
        (
            exception.rule.as_str(),
            exception.rule_package.as_str(),
            exception.package.as_str(),
            exception.target.as_str()
        ),
        ("NoDocument", "org.standards", "example.shop", "Open")
    );
}

#[test]
fn an_importer_cannot_shadow_an_imported_rule() {
    let dir = scratch("shadow");
    standards(&dir);
    shop(&dir, "\"org.standards\"", "rule Bounded { no document }");
    mentions(
        &errors(&compiled(&dir)),
        "rule `Bounded` shadows the rule of that name in `org.standards`",
    );
}

#[test]
fn what_is_not_imported_is_not_loaded() {
    let dir = scratch("unimported");
    standards(&dir);
    // Declared as a dependency, never imported: neither its rules nor its
    // metadata exist for the package.
    shop(&dir, "", "");
    let found = errors(&compiled(&dir));
    mentions(&found, "unknown annotation `@owned`");
    assert!(
        found.iter().all(|m| !m.contains("org.standards#")),
        "{found:#?}"
    );
}

#[test]
fn dependencies_import_dependencies() {
    let dir = scratch("transitive");
    standards(&dir);
    write(
        &dir,
        &[
            (
                "base/crucible.toml",
                "[model]\nnamespace = \"org.base\"\nsources = [\"policy\"]\nimports = [\"org.standards\"]\n\n[model.dependencies]\n\"org.standards\" = { path = \"../standards\" }\n",
            ),
            (
                "base/policy/base.cru",
                "/// No booleans.\nrule NoBooleans { no boolean }\n",
            ),
        ],
    );
    write(
        &dir,
        &[
            (
                "shop/crucible.toml",
                "[model]\nnamespace = \"example.shop\"\nsources = [\"model\"]\nimports = [\"org.base\"]\n\n[model.dependencies]\n\"org.base\" = { path = \"../base\" }\n",
            ),
            ("shop/model/shop.cru", SHOP),
        ],
    );
    let package = load_package(&dir.join("shop")).unwrap();
    let names: Vec<&str> = package
        .dependencies
        .iter()
        .map(|d| d.namespace.as_str())
        .collect();
    assert_eq!(names, ["org.standards", "org.base"]);
    // The standards' rules reach the shop through the base package.
    let found = errors(&compile(&package, None));
    mentions(&found, "rule org.standards#Bounded: `Loose`");
}

#[test]
fn the_manifest_is_checked() {
    let dir = scratch("manifest");
    standards(&dir);
    // The package directory exists: `shop/../standards` needs it to resolve.
    std::fs::create_dir_all(dir.join("shop")).unwrap();
    let check = |manifest: &str| {
        check_manifest(&dir.join("shop"), manifest)
            .into_iter()
            .map(|d| d.message)
            .collect::<Vec<_>>()
    };
    let base =
        "[model]\nnamespace = \"example.shop\"\nsources = []\nimports = [\"org.standards\"]\n";
    mentions(
        &check(&format!(
            "{base}[model.dependencies]\n\"org.standards\" = {{ path = \"../nowhere\" }}\n"
        )),
        "is not a package",
    );
    mentions(
        &check(&format!(
            "{base}[model.dependencies]\n\"org.other\" = {{ path = \"../standards\" }}\n"
        )),
        "declares the namespace `org.standards`, not `org.other`",
    );
    mentions(
        &check(&format!(
            "{base}[model.dependencies]\n\"org.standards\" = {{ path = \"../standards\", version = \"1\" }}\n"
        )),
        "unknown key `version`",
    );
    mentions(
        &check(&format!(
            "{base}[model.dependencies]\n\"org.standards\" = \"../standards\"\n"
        )),
        "is a dependency: write",
    );
    mentions(
        &check(&format!(
            "{base}[model.dependencies]\n\"org.standards\" = {{}}\n"
        )),
        "needs a `path`",
    );
    mentions(
        &check(&format!(
            "{base}[model.dependencies]\n\"crucible.errors\" = {{ path = \"../standards\" }}\n"
        )),
        "reserved",
    );
    // An import that is neither bundled nor a dependency.
    mentions(&check(base), "unknown import `org.standards`");
    // The entry is well formed.
    let ok = check(&format!(
        "{base}[model.dependencies]\n\"org.standards\" = {{ path = \"../standards\" }}\n"
    ));
    assert!(ok.is_empty(), "{ok:?}");
}

#[test]
fn a_dependency_cycle_is_refused() {
    let dir = scratch("cycle");
    write(
        &dir,
        &[
            (
                "a/crucible.toml",
                "[model]\nnamespace = \"org.a\"\nsources = [\"m\"]\nimports = [\"org.b\"]\n[model.dependencies]\n\"org.b\" = { path = \"../b\" }\n",
            ),
            ("a/m/a.cru", "string A\n"),
            (
                "b/crucible.toml",
                "[model]\nnamespace = \"org.b\"\nsources = [\"m\"]\nimports = [\"org.a\"]\n[model.dependencies]\n\"org.a\" = { path = \"../a\" }\n",
            ),
            ("b/m/b.cru", "string B\n"),
        ],
    );
    let error = load_package(&dir.join("a")).unwrap_err();
    assert!(error.contains("a cycle"), "{error}");
}

#[test]
fn a_package_reached_two_ways_loads_once() {
    let dir = scratch("diamond");
    standards(&dir);
    write(
        &dir,
        &[
            (
                "base/crucible.toml",
                "[model]\nnamespace = \"org.base\"\nsources = [\"policy\"]\nimports = [\"org.standards\"]\n\n[model.dependencies]\n\"org.standards\" = { path = \"../standards\" }\n",
            ),
            (
                "base/policy/base.cru",
                "/// No booleans.\nrule NoBooleans { no boolean }\n",
            ),
            (
                "shop/crucible.toml",
                // The same package by a different spelling of its path.
                "[model]\nnamespace = \"example.shop\"\nsources = [\"model\"]\nimports = [\"org.base\", \"org.standards\"]\n\n[model.dependencies]\n\"org.base\" = { path = \"../base\" }\n\"org.standards\" = { path = \"../base/../standards\" }\n",
            ),
            ("shop/model/shop.cru", SHOP),
        ],
    );
    let package = load_package(&dir.join("shop")).unwrap();
    let names: Vec<&str> = package
        .dependencies
        .iter()
        .map(|d| d.namespace.as_str())
        .collect();
    assert_eq!(names, ["org.standards", "org.base"]);
}

#[test]
fn two_dependencies_cannot_share_a_rule_name() {
    let dir = scratch("same-rule");
    standards(&dir);
    write(
        &dir,
        &[
            (
                "other/crucible.toml",
                "[model]\nnamespace = \"org.other\"\nsources = [\"policy\"]\n",
            ),
            ("other/policy/other.cru", "rule Bounded { no boolean }\n"),
            (
                "shop/crucible.toml",
                "[model]\nnamespace = \"example.shop\"\nsources = [\"model\"]\nimports = [\"org.standards\", \"org.other\"]\n\n[model.dependencies]\n\"org.standards\" = { path = \"../standards\" }\n\"org.other\" = { path = \"../other\" }\n",
            ),
            ("shop/model/shop.cru", SHOP),
        ],
    );
    mentions(
        &errors(&compiled(&dir)),
        "is declared by both `org.standards` and `org.other`",
    );
}

/// A package whose migration's documentation names its own older release.
const NOTES: &str = r#"
string Note

/// v2 drops [Gone], which only this package's v1 had.
migration "v2" from "v1" {
    /// [Gone] is retired.
    sunset "2027-01-01"
}
"#;

#[test]
fn an_imported_packages_migration_docs_are_its_own() {
    let dir = scratch("imported-migration-docs");
    write(
        &dir,
        &[
            (
                "notes/crucible.toml",
                "[model]\nnamespace = \"org.notes\"\n",
            ),
            ("notes/notes.cru", NOTES),
            (
                "shop/crucible.toml",
                "[model]\nnamespace = \"example.shop\"\nimports = [\"org.notes\"]\n\n[model.dependencies]\n\"org.notes\" = { path = \"../notes\" }\n",
            ),
            ("shop/shop.cru", SHOP.replace("@owned\n", "").as_str()),
        ],
    );
    // The consumer has neither `Gone` nor the import's releases: the
    // import's migration is not checked against them.
    let shop = compiled(&dir);
    assert!(shop.service.is_some(), "{}", shop.render());
    assert!(
        !shop.service.unwrap().doc_links.contains_key("Gone"),
        "an imported migration's link reaches no table"
    );
    // Its own compile checks it.
    let notes = compile(&load_package(&dir.join("notes")).unwrap(), None);
    assert!(
        notes
            .render()
            .contains("the doc link `[Gone]` names nothing"),
        "{}",
        notes.render()
    );
}
