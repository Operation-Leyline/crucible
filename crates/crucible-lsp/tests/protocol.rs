//! Drives the real `crucible-lsp` binary over standard I/O, the way an
//! editor does: open a package file, read diagnostics, then ask for
//! highlighting, a definition, hover and the outline.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl Client {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_crucible-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut client = Self {
            child,
            stdin,
            stdout,
            next_id: 0,
        };
        let capabilities = client.request("initialize", json!({ "capabilities": {} }));
        assert!(capabilities["capabilities"]["semanticTokensProvider"].is_object());
        client.notify("initialized", json!({}));
        client
    }

    fn send(&mut self, message: Value) {
        let body = message.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn receive(&mut self) -> Value {
        let mut length = 0;
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).unwrap();
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = value.parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        self.stdout.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let message = self.receive();
            if message["id"] == id {
                return message["result"].clone();
            }
        }
    }

    /// The next diagnostics published for `uri`.
    fn diagnostics(&mut self, uri: &str) -> Vec<Value> {
        loop {
            let message = self.receive();
            if message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == uri
            {
                return message["params"]["diagnostics"].as_array().unwrap().clone();
            }
        }
    }

    fn open(&mut self, uri: &str, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri, "languageId": "crucible", "version": 1, "text": text } }),
        );
    }

    fn change(&mut self, uri: &str, version: i32, text: &str) {
        self.notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text }] }),
        );
    }

    fn stop(mut self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        assert!(self.child.wait().unwrap().success());
    }
}

const SERVICE: &str = r#"service Shop {
    version "1"
    protocol json
    operations [GetItem]
    errors [ValidationException, InternalServerException]
}

/// Reads one item.
readonly operation GetItem {
    http GET "/items/{id}"
    input { id: ItemId }
    output { name: string }
}
"#;

const TYPES: &str = "/// An item's identifier.\nstring ItemId { length 1..10 }\n";

fn package(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("crucible.toml"),
        "[model]\nnamespace = \"example.shop\"\nimports = [\"crucible.errors\"]\n",
    )
    .unwrap();
    std::fs::write(dir.join("service.cru"), SERVICE).unwrap();
    std::fs::write(dir.join("types.cru"), TYPES).unwrap();
}

fn uri(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
}

/// The 0-based line and UTF-16 column of the `nth` occurrence of `word`.
fn position(text: &str, word: &str, nth: usize) -> Value {
    let offset = text.match_indices(word).nth(nth).unwrap().0;
    let line = text[..offset].matches('\n').count();
    let column = offset - text[..offset].rfind('\n').map_or(0, |index| index + 1);
    json!({ "line": line, "character": column })
}

#[test]
fn an_editor_session_gets_diagnostics_highlighting_and_navigation() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-session");
    package(&dir);
    let service = uri(&dir.join("service.cru"));
    let mut client = Client::start();

    client.open(&service, SERVICE);
    assert_eq!(client.diagnostics(&service), Vec::<Value>::new());

    // A typo is reported as the user types, at the misspelled name, and the
    // package's other files and imports take part in the check.
    let broken = SERVICE.replace("id: ItemId", "id: ItemIdd");
    client.change(&service, 2, &broken);
    let diagnostics = client.diagnostics(&service);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .unwrap()
            .contains("ItemIdd"),
        "{diagnostics:#?}"
    );
    assert_eq!(
        diagnostics[0]["range"]["start"],
        position(&broken, "ItemIdd", 0)
    );
    client.change(&service, 3, SERVICE);
    assert_eq!(client.diagnostics(&service), Vec::<Value>::new());

    let tokens = client.request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": service } }),
    );
    let data = tokens["data"].as_array().unwrap();
    assert!(data.len() > 20 && data.len() % 5 == 0, "{tokens}");

    // `ItemId` is declared in the other file.
    let definition = client.request(
        "textDocument/definition",
        json!({ "textDocument": { "uri": service }, "position": position(SERVICE, "ItemId", 0) }),
    );
    assert!(
        definition["uri"]
            .as_str()
            .unwrap_or_else(|| panic!("no definition: {definition}"))
            .to_lowercase()
            .ends_with("types.cru"),
        "{definition}"
    );
    assert_eq!(definition["range"]["start"], position(TYPES, "ItemId", 0));

    let hover = client.request(
        "textDocument/hover",
        json!({ "textDocument": { "uri": service }, "position": position(SERVICE, "ItemId", 0) }),
    );
    let text = hover["contents"]["value"].as_str().unwrap();
    assert!(
        text.contains("string ItemId") && text.contains("An item's identifier."),
        "{text}"
    );

    // Imported declarations hover too, though they have no file to open.
    let hover = client.request(
        "textDocument/hover",
        json!({ "textDocument": { "uri": service }, "position": position(SERVICE, "ValidationException", 0) }),
    );
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("error ValidationException")
    );

    let symbols = client.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": service } }),
    );
    let names: Vec<&str> = symbols
        .as_array()
        .unwrap()
        .iter()
        .map(|symbol| symbol["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Shop", "GetItem"]);

    client.stop();
}

#[test]
fn a_file_outside_any_package_is_checked_on_its_own() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-loose");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("scratch.cru");
    let text = "structure A { b: Missing }\n";
    std::fs::write(&file, text).unwrap();
    let file = uri(&file);
    let mut client = Client::start();
    client.open(&file, text);
    let diagnostics = client.diagnostics(&file);
    // "No service" is about packages; the unknown type is the real problem.
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .unwrap()
            .contains("Missing")
    );
    client.stop();
}

fn labels(items: &Value) -> Vec<String> {
    items
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["label"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn editing_features_work_across_the_package() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-editing");
    package(&dir);
    let service = uri(&dir.join("service.cru"));
    let mut client = Client::start();
    client.open(&service, SERVICE);
    assert_eq!(client.diagnostics(&service), Vec::<Value>::new());

    // Completion after `id: ` offers the package's types and the built-ins.
    let typing = SERVICE.replace("id: ItemId", "id: ");
    client.change(&service, 2, &typing);
    client.diagnostics(&service);
    let at = position(&typing, "id: ", 0);
    let at = json!({ "line": at["line"], "character": at["character"].as_u64().unwrap() + 4 });
    let items = client.request(
        "textDocument/completion",
        json!({ "textDocument": { "uri": service }, "position": at }),
    );
    let offered = labels(&items);
    assert!(
        offered.contains(&"ItemId".to_owned()) && offered.contains(&"string".to_owned()),
        "{offered:?}"
    );
    client.change(&service, 3, SERVICE);
    client.diagnostics(&service);

    // References span both files; highlights stay in this one.
    let on_type =
        json!({ "textDocument": { "uri": service }, "position": position(SERVICE, "ItemId", 0) });
    let mut with_declaration = on_type.clone();
    with_declaration["context"] = json!({ "includeDeclaration": true });
    let references = client.request("textDocument/references", with_declaration);
    let files: Vec<String> = references
        .as_array()
        .unwrap()
        .iter()
        .map(|location| location["uri"].as_str().unwrap().to_lowercase())
        .collect();
    assert_eq!(files.len(), 2, "{references}");
    assert!(
        files.iter().any(|file| file.ends_with("types.cru"))
            && files.iter().any(|file| file.ends_with("service.cru"))
    );
    let highlights = client.request("textDocument/documentHighlight", on_type.clone());
    assert_eq!(highlights.as_array().unwrap().len(), 1, "{highlights}");

    // Rename edits every file; names that clash or come from imports are refused.
    let prepared = client.request("textDocument/prepareRename", on_type.clone());
    assert_eq!(prepared["placeholder"], "ItemId");
    let mut rename = on_type.clone();
    rename["newName"] = json!("ItemKey");
    let edit = client.request("textDocument/rename", rename.clone());
    let changes = edit["changes"].as_object().unwrap();
    assert_eq!(changes.len(), 2, "{edit}");
    assert!(
        changes
            .values()
            .all(|edits| edits[0]["newText"] == "ItemKey")
    );
    rename["newName"] = json!("GetItem");
    client.send(
        json!({ "jsonrpc": "2.0", "id": 900, "method": "textDocument/rename", "params": rename }),
    );
    let refused = loop {
        let message = client.receive();
        if message["id"] == 900 {
            break message;
        }
    };
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("already declared"),
        "{refused}"
    );
    let imported = client.request(
        "textDocument/prepareRename",
        json!({ "textDocument": { "uri": service }, "position": position(SERVICE, "ValidationException", 0) }),
    );
    assert!(imported.is_null(), "{imported}");

    let folds = client.request(
        "textDocument/foldingRange",
        json!({ "textDocument": { "uri": service } }),
    );
    assert!(folds.as_array().unwrap().len() >= 2, "{folds}");

    // Formatting restores the house style.
    let messy = SERVICE.replace("\n    ", "\n");
    client.change(&service, 4, &messy);
    client.diagnostics(&service);
    let edits = client.request(
        "textDocument/formatting",
        json!({ "textDocument": { "uri": service }, "options": { "tabSize": 4, "insertSpaces": true } }),
    );
    assert_eq!(edits[0]["newText"], SERVICE, "{edits}");

    // A misspelt type offers the close name and a declaration.
    let broken = SERVICE.replace("id: ItemId", "id: ItemIdd");
    client.change(&service, 5, &broken);
    let diagnostics = client.diagnostics(&service);
    let actions = client.request(
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": service },
            "range": diagnostics[0]["range"],
            "context": { "diagnostics": diagnostics },
        }),
    );
    let titles: Vec<&str> = actions
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        ["Change to `ItemId`", "Declare structure `ItemIdd`"],
        "{actions}"
    );
    client.change(&service, 6, SERVICE);
    client.diagnostics(&service);

    let symbols = client.request("workspace/symbol", json!({ "query": "geti" }));
    assert!(
        symbols
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "GetItem"),
        "{symbols}"
    );

    // The manifest is checked as it is edited.
    let manifest = uri(&dir.join("crucible.toml"));
    client.open(
        &manifest,
        "[model]\nnamespace = \"example.shop\"\nimports = [\"crucible.errors\"]\nnamspace = \"x\"\n",
    );
    let problems = client.diagnostics(&manifest);
    assert!(
        problems[0]["message"]
            .as_str()
            .unwrap()
            .contains("namspace"),
        "{problems:?}"
    );
    assert_eq!(problems[0]["range"]["start"]["line"], 3);

    client.stop();
}

#[test]
fn an_untitled_buffer_is_checked() {
    let mut client = Client::start();
    let uri = "untitled:Untitled-1";
    client.open(uri, "structure A { b: Missing }\n");
    let diagnostics = client.diagnostics(uri);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .unwrap()
            .contains("Missing")
    );
    client.stop();
}

#[test]
fn a_burst_of_edits_ends_with_the_last_texts_diagnostics() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-burst");
    package(&dir);
    let service = uri(&dir.join("service.cru"));
    let mut client = Client::start();
    client.open(&service, SERVICE);
    client.diagnostics(&service);
    for version in 2..30 {
        client.change(
            &service,
            version,
            &SERVICE.replace("id: ItemId", &format!("id: Typo{version}")),
        );
    }
    client.change(&service, 30, SERVICE);
    // Intermediate texts may or may not be compiled; the last one must be.
    let mut last = client.diagnostics(&service);
    for _ in 0..30 {
        if last.is_empty() {
            break;
        }
        last = client.diagnostics(&service);
    }
    assert_eq!(last, Vec::<Value>::new());
    client.stop();
}
