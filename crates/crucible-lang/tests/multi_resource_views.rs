//! A view declared `for A, B` serves several resources: what it shows is
//! lowered against the first and must be declared identically by each of the
//! others (decision 34).

use crucible_lang::{Compiled, FormatOptions, Package, SourceFile, compile, format, parse};
use crucible_smithy::ir::{Member, Mutability, Service, Shape};

const SERVICE: &str = r#"
service Jobs {
    version "1"
    protocol json
    auth bearer
    resources [Job, HistoryJob]
}

string JobId
string Statement { length 1..100 }
integer Count { range 0.. }
string Reason { length 1..50 }

resource Job {
    id jobId: JobId
    immutable property statement: Statement
    readonly property count: Count
    readonly property reason?: Reason
}

resource HistoryJob {
    id jobId: JobId
    immutable property statement: Statement
    readonly property count: Count
    readonly property reason?: Reason
}

operation CreateJob creates Job {
    http POST "/jobs"
    input { $statement }
    output { job: JobData }
}

operation GetJob reads Job {
    http GET "/jobs/{jobId}"
    input { $jobId }
    output { job: JobData }
}

operation CreateHistoryJob creates HistoryJob {
    http POST "/history-jobs"
    input { $statement }
    output { job: JobData }
}

operation GetHistoryJob reads HistoryJob {
    http GET "/history-jobs/{jobId}"
    input { $jobId }
    output { job: JobData }
}
"#;

/// The operation that sets a `Draft`'s immutable property.
const CREATE_DRAFT: &str = r#"
operation CreateDraft creates Draft {
    http POST "/drafts"
    input { $statement }
}
"#;

fn compiled(extra: &str) -> Compiled {
    let package = Package::new(
        "example.jobs",
        vec![SourceFile {
            path: "jobs.cru".to_owned(),
            text: if extra.contains("resource Draft") {
                // Reached, with the operation that sets its immutable property.
                format!(
                    "{}
{extra}{CREATE_DRAFT}",
                    SERVICE.replace("[Job, HistoryJob]", "[Job, HistoryJob, Draft]")
                )
            } else {
                format!(
                    "{SERVICE}
{extra}"
                )
            },
        }],
    );
    compile(&package, None)
}

fn problems(extra: &str) -> (Vec<String>, Vec<String>) {
    let compiled = compiled(extra);
    let (mut errors, mut warnings) = (Vec::new(), Vec::new());
    for item in compiled.diagnostics {
        if item.diagnostic.is_error() {
            errors.push(item.diagnostic.message);
        } else {
            warnings.push(item.diagnostic.message);
        }
    }
    (errors, warnings)
}

fn service(extra: &str) -> Service {
    let compiled = compiled(extra);
    assert!(!compiled.has_errors(), "{}", compiled.render());
    compiled.service.expect("service")
}

fn members<'a>(service: &'a Service, shape: &str) -> &'a [Member] {
    match service.shapes.iter().find(|(id, _)| id.name() == shape) {
        Some((_, Shape::Structure(s))) => &s.members,
        _ => panic!("no structure {shape}"),
    }
}

fn mentions(messages: &[String], needle: &str) {
    assert!(
        messages.iter().any(|m| m.contains(needle)),
        "no message mentions {needle:?}: {messages:#?}"
    );
}

#[test]
fn a_view_of_two_identical_resources_compiles_without_warnings() {
    let extra = "structure JobData for Job, HistoryJob { $jobId\n $statement\n $count\n $reason }";
    let (errors, warnings) = problems(extra);
    assert!(errors.is_empty(), "{errors:?}");
    assert!(warnings.is_empty(), "{warnings:?}");
    let service = service(extra);
    let shown: Vec<_> = members(&service, "JobData")
        .iter()
        .map(|m| (m.name.as_str(), m.required, m.mutability))
        .collect();
    assert_eq!(
        shown,
        [
            ("jobId", true, None),
            ("statement", true, Some(Mutability::CreateAndRead)),
            ("count", true, Some(Mutability::Read)),
            ("reason", false, Some(Mutability::Read)),
        ]
    );
}

#[test]
fn a_single_resource_view_does_not_count_for_the_other_resource() {
    // The warning the shared view exists to remove.
    let (errors, warnings) = problems("structure JobData for Job { $* }");
    assert!(errors.is_empty(), "{errors:?}");
    mentions(&warnings, "HistoryJob");
}

#[test]
fn star_shows_what_every_resource_declares() {
    let extra = r#"
resource Draft {
    id jobId: JobId
    immutable property statement: Statement
    property comment?: Reason
}
structure JobData for Job, Draft { $* }
"#;
    // `Draft` is not in the service, so only the structure is checked.
    let (errors, _) = problems(extra);
    assert!(errors.is_empty(), "{errors:?}");
    let service = service(extra);
    let names: Vec<_> = members(&service, "JobData")
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    // `count` and `reason` are Job's alone; `comment` is Draft's alone.
    assert_eq!(names, ["jobId", "statement"]);
}

#[test]
fn a_member_the_other_resource_lacks_is_an_error() {
    let extra = r#"
resource Draft {
    id jobId: JobId
    immutable property statement: Statement
}
structure JobData for Job, Draft { $jobId $statement $count }
"#;
    let (errors, _) = problems(extra);
    mentions(
        &errors,
        "resource `Draft` has no identifier or property `count`",
    );
}

#[test]
fn members_that_differ_between_resources_are_errors_that_name_the_difference() {
    let extra = r#"
resource Draft {
    id jobId: JobId as id
    immutable property statement: Statement
    property count: Count
    readonly property reason: Reason
}
structure JobData for Job, Draft { $jobId $statement $count $reason }
"#;
    let (errors, _) = problems(extra);
    // A different spelling of the identifier.
    mentions(
        &errors,
        "`jobId` is not the same member of `Job` and `Draft` (name",
    );
    // Different mutability.
    mentions(
        &errors,
        "`count` is not the same member of `Job` and `Draft` (mutability",
    );
    // Different presence.
    mentions(
        &errors,
        "`reason` is not the same member of `Job` and `Draft` (required",
    );
    assert_eq!(errors.len(), 3, "{errors:?}");
}

#[test]
fn a_different_type_or_constraint_is_a_difference() {
    let extra = r#"
string Short { length 1..5 }
resource Draft {
    id jobId: JobId
    immutable property statement: Short
}
structure JobData for Job, Draft { $jobId $statement }
"#;
    let (errors, _) = problems(extra);
    mentions(
        &errors,
        "`statement` is not the same member of `Job` and `Draft` (target",
    );
}

#[test]
fn an_explicit_spelling_makes_a_renamed_identifier_agree() {
    let extra = r#"
resource Draft {
    id jobId: JobId as id
    immutable property statement: Statement
}
structure JobData for Job, Draft { $jobId as id  $statement }
"#;
    let (errors, _) = problems(extra);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn the_other_resources_are_checked() {
    let (errors, _) = problems("structure JobData for Job, Missing { $jobId }");
    mentions(&errors, "`Missing` is not a resource");
    let (errors, _) = problems("structure JobData for Job, Job { $jobId }");
    mentions(&errors, "`Job` is named twice");
    let (errors, _) = problems("structure JobData for Job, HistoryJob, HistoryJob { $jobId }");
    mentions(&errors, "`HistoryJob` is named twice");
}

#[test]
fn an_included_view_keeps_checking_every_resource() {
    let extra = r#"
resource Draft {
    id jobId: JobId
    property count: Count
}
structure Core for Job, Draft { $jobId }
structure More for Job, Draft { include Core  $count }
structure JobData for Job { $jobId }
"#;
    let (errors, _) = problems(extra);
    mentions(
        &errors,
        "`count` is not the same member of `Job` and `Draft`",
    );
}

#[test]
fn the_for_list_formats_and_round_trips() {
    let source = "structure JobData for Job, HistoryJob { $jobId }\n";
    let formatted = format(source, &FormatOptions::default());
    assert_eq!(formatted, source);
    let parsed = parse(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
}

#[test]
fn different_entry_documentation_is_not_a_difference() {
    let extra = r#"
resource Draft {
    id jobId: JobId
    /// What the draft is for.
    immutable property statement: Statement
}
structure JobData for Job, Draft { $jobId $statement }
"#;
    let (errors, _) = problems(extra);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn a_view_included_twice_over_reports_a_mismatch_once_per_place() {
    let extra = r#"
resource Draft {
    id jobId: JobId
    property count: Count
}
structure Core for Job, Draft { $count }
structure JobData for Job, Draft { include Core }
"#;
    let compiled = compiled(extra);
    let mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|item| item.diagnostic.message.contains("is not the same member"))
        .map(|item| {
            (
                item.file,
                item.diagnostic.span,
                item.diagnostic.message.clone(),
            )
        })
        .collect();
    let mut unique = mismatches.clone();
    unique.sort_by_key(|(file, span, _)| (*file, span.start, span.end));
    unique.dedup();
    assert_eq!(mismatches.len(), unique.len(), "{mismatches:#?}");
}

#[test]
fn a_shared_view_included_in_a_creating_input_binds_the_first_resource_only() {
    // The view is lowered against `Job`, so in a creating input of `Note` it
    // binds nothing there: `statement` is still never set (34a).
    let shared = r#"
resource Note {
    id jobId: JobId
    immutable property statement: Statement
}
structure StatementView for Job, Note { $statement }
operation CreateNote creates Note {
    http POST "/notes"
    input { include StatementView }
}
"#;
    let (errors, _) = problems(shared);
    mentions(&errors, "nothing could ever set it");
    // A view of `Note` alone binds it.
    let own = shared.replace("for Job, Note", "for Note");
    let (errors, _) = problems(&own);
    assert!(
        errors
            .iter()
            .all(|m| !m.contains("nothing could ever set it")),
        "{errors:?}"
    );
}
