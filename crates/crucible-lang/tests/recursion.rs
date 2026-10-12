//! Recursive shapes (Smithy 2.0, "Recursive shape definitions"): allowed,
//! recorded in `recursive_groups` as the Smithy front end records them, and
//! refused only where no finite value exists or only collections recur.

use crucible_lang::{Compiled, Package, SourceFile, compile};
use crucible_smithy::ir::{Service, Shape, Structure};

const RECURSIVE: &str = include_str!("fixtures/recursive.cru");

fn compiled(text: &str) -> Compiled {
    let package = Package {
        namespace: "example.search".to_owned(),
        files: vec![SourceFile {
            path: "search.cru".to_owned(),
            text: text.to_owned(),
        }],
        naming: Default::default(),
        imports: Vec::new(),
        baseline: None,
    };
    compile(&package, None)
}

fn service(text: &str) -> Service {
    let compiled = compiled(text);
    let messages: Vec<_> = compiled
        .diagnostics
        .iter()
        .map(|d| &d.diagnostic.message)
        .collect();
    compiled
        .service
        .unwrap_or_else(|| panic!("no service: {messages:#?}"))
}

fn messages(text: &str) -> Vec<String> {
    compiled(text)
        .diagnostics
        .into_iter()
        .map(|d| d.diagnostic.message)
        .collect()
}

/// A service whose one operation outputs `output`, around `shapes`.
fn with_shapes(shapes: &str, output: &str) -> String {
    format!(
        "service Search {{\n    version \"1\"\n    protocol json\n    operations [Find]\n    errors [Invalid]\n}}\n\
         error Invalid {{\n    status 400\n    message: string\n}}\n\
         {shapes}\n\
         operation Find {{\n    http POST \"/find\"\n    output {{\n        {output}\n    }}\n}}\n"
    )
}

fn assert_mentions(messages: &[String], needle: &str) {
    assert!(
        messages.iter().any(|m| m.contains(needle)),
        "no diagnostic mentions {needle:?}: {messages:#?}"
    );
}

fn structure<'a>(service: &'a Service, name: &str) -> &'a Structure {
    match service.shapes.iter().find(|(id, _)| id.name() == name) {
        Some((_, Shape::Structure(structure))) => structure,
        other => panic!("{name}: {other:?}"),
    }
}

fn names(ids: &[crucible_smithy::ShapeId]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
}

#[test]
fn recursive_shapes_lower_and_record_their_groups() {
    let service = service(RECURSIVE);
    let groups: Vec<(Vec<String>, Vec<String>)> = service
        .recursive_groups
        .iter()
        .map(|group| (names(&group.shapes), names(&group.members)))
        .collect();
    assert_eq!(
        groups,
        [
            (
                vec![
                    "example.search#Filter".to_owned(),
                    "example.search#Filters".to_owned()
                ],
                vec![
                    "example.search#Filter$all".to_owned(),
                    "example.search#Filter$not".to_owned(),
                    "example.search#Filters$member".to_owned(),
                ]
            ),
            (
                vec![
                    "example.search#Forest".to_owned(),
                    "example.search#Grove".to_owned(),
                    "example.search#Tree".to_owned()
                ],
                vec![
                    "example.search#Forest$member".to_owned(),
                    "example.search#Grove$value".to_owned(),
                    "example.search#Tree$children".to_owned(),
                    "example.search#Tree$named".to_owned(),
                ]
            ),
        ]
    );
}

/// The Smithy front end's groups for the same shapes: one algorithm, so the
/// two cannot disagree about which members recur.
#[test]
fn the_groups_are_the_ones_the_shared_algorithm_finds() {
    let service = service(RECURSIVE);
    assert_eq!(
        service.recursive_groups,
        crucible_smithy::ir::recursive_groups(&service.shapes)
    );
}

/// A member that reaches a list while the list is still being lowered (the
/// output names `Forest` first, so `Tree.children` meets it mid-lowering)
/// must still carry the list's constraints, as when `Tree` comes first.
#[test]
fn a_member_reaching_a_list_mid_lowering_keeps_its_constraints() {
    let shapes = "structure Tree {\n    label: string\n    children?: Forest\n}\n\
                  list Forest: Tree { length ..10 }\n";
    for output in ["forest: Forest", "tree: Tree"] {
        let service = service(&with_shapes(shapes, output));
        let children = structure(&service, "Tree").member("children").unwrap();
        let length = children.constraints.length.as_ref();
        assert_eq!(length.and_then(|l| l.max), Some(10), "output {output}");
    }
}

#[test]
fn a_structure_recurring_only_through_required_members_is_refused() {
    let m = messages(&with_shapes(
        "structure Node {\n    next: Node\n}\n",
        "node?: Node",
    ));
    assert_mentions(&m, "reaches itself through required members alone");
    let m = messages(&with_shapes(
        "structure A {\n    b: B\n}\nstructure B {\n    a: A\n}\n",
        "a?: A",
    ));
    assert_eq!(
        m.iter()
            .filter(|m| m.contains("reaches itself through required members alone"))
            .count(),
        2,
        "{m:#?}"
    );
    // One optional member is a way out.
    service(&with_shapes(
        "structure A {\n    b: B\n}\nstructure B {\n    a?: A\n}\n",
        "a?: A",
    ));
}

/// Smithy's validator, not a stricter reading: a structure with an optional
/// member terminates for the union check, though its required member leads
/// back. Smithy accepts this model, so both front ends must.
#[test]
fn recursion_smithy_accepts_is_accepted() {
    service(&with_shapes(
        "structure S {\n    r: U\n    o?: string\n}\nunion U {\n    s: S\n}\n",
        "s?: S",
    ));
}

#[test]
fn a_union_needs_a_member_that_does_not_lead_back() {
    let m = messages(&with_shapes(
        "union Loop {\n    again: Loop\n    wrapped: Wrap\n}\nstructure Wrap {\n    inner: Loop\n}\n",
        "loop?: Loop",
    ));
    assert_mentions(&m, "no finite value of this union exists");
    // A list is a way out: it may be empty.
    service(&with_shapes(
        "union Loop {\n    again: Loop\n    many: Loops\n}\nlist Loops: Loop\n",
        "loop?: Loop",
    ));
}

#[test]
fn a_cycle_of_collections_alone_is_refused() {
    let m = messages(&with_shapes("list Nested: Nested\n", "nested?: Nested"));
    assert_mentions(&m, "collection-only cycle");
}

#[test]
fn a_default_on_a_recursive_structure_member_is_still_refused() {
    let m = messages(&with_shapes(
        "structure Node {\n    next?: Node = {}\n}\n",
        "node?: Node",
    ));
    assert_mentions(&m, "a structure has no default");
}
