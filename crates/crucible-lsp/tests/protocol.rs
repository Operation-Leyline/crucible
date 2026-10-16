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
    /// What the server said it can do.
    capabilities: Value,
}

impl Client {
    fn start() -> Self {
        Self::start_with(json!({ "capabilities": {} }))
    }

    /// Workspace symbol names for `query`, asked again until `done` holds
    /// or a few seconds pass: packages compile in the background.
    fn symbols_eventually(
        &mut self,
        query: &str,
        done: impl Fn(&Vec<String>) -> bool,
    ) -> Vec<String> {
        let mut names = Vec::new();
        for _ in 0..100 {
            let symbols = self.request("workspace/symbol", json!({ "query": query }));
            names = symbols
                .as_array()
                .unwrap()
                .iter()
                .map(|symbol| symbol["name"].as_str().unwrap().to_owned())
                .collect();
            if done(&names) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        names
    }

    fn start_with(params: Value) -> Self {
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
            capabilities: Value::Null,
        };
        let capabilities = client.request("initialize", params);
        client.capabilities = capabilities["capabilities"].clone();
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
        self.respond(method, params)["result"].clone()
    }

    /// The error message a request is refused with.
    fn request_error(&mut self, method: &str, params: Value) -> String {
        let reply = self.respond(method, params);
        reply["error"]["message"]
            .as_str()
            .unwrap_or_else(|| panic!("{method} succeeded: {reply}"))
            .to_owned()
    }

    fn respond(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let message = self.receive();
            if message["id"] == id {
                return message;
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

#[test]
fn unsaved_package_sources_have_diagnostics_and_symbols() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-unsaved-source");
    package(&dir);
    let service_uri = uri(&dir.join("service.cru"));
    let new_uri = uri(&dir.join("new.cru"));
    let new_text =
        "string NewId { length 1..10 }\nstructure NewThing { value: DefinitelyMissing }\n";
    let mut client = Client::start();
    client.open(&service_uri, SERVICE);
    assert!(client.diagnostics(&service_uri).is_empty());
    client.open(&new_uri, new_text);
    let diagnostics = client.diagnostics(&new_uri);
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("DefinitelyMissing")),
        "{diagnostics:#?}"
    );
    let symbols = client.request("workspace/symbol", json!({ "query": "NewThing" }));
    assert_eq!(symbols.as_array().unwrap().len(), 1, "{symbols}");
    client.change(&service_uri, 2, &SERVICE.replace("ItemId", "NewId"));
    let diagnostics = client.diagnostics(&service_uri);
    assert!(
        !diagnostics.iter().any(|diagnostic| diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("unknown type `NewId`")),
        "{diagnostics:#?}"
    );
    client.stop();
}

#[test]
fn unsaved_sources_in_new_directories_belong_to_their_package() {
    for (name, missing_root) in [("missing-root", true), ("new-subdirectory", false)] {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("lsp-unsaved-{name}"));
        package(&dir);
        let missing = if missing_root {
            "model"
        } else {
            "new/subdirectory"
        };
        if missing_root {
            std::fs::write(
                dir.join("crucible.toml"),
                "[model]\nnamespace = \"example.shop\"\nsources = [\".\", \"model\"]\nimports = [\"crucible.errors\"]\n",
            )
            .unwrap();
        }
        let new_uri = uri(&dir.join(missing).join("new.cru"));
        let mut client = Client::start();
        client.open(
            &new_uri,
            "string NewId\nstructure NewThing { value: DefinitelyMissing }\n",
        );
        let diagnostics = client.diagnostics(&new_uri);
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic["message"]
                .as_str()
                .unwrap()
                .contains("DefinitelyMissing")),
            "{name}: {diagnostics:#?}"
        );
        assert!(
            !diagnostics.iter().any(|diagnostic| diagnostic["message"]
                .as_str()
                .unwrap()
                .contains("declares no service")),
            "{name}: {diagnostics:#?}"
        );
        let symbols = client.request("workspace/symbol", json!({ "query": "NewThing" }));
        assert_eq!(symbols.as_array().unwrap().len(), 1, "{name}: {symbols}");
        assert_eq!(
            symbols[0]["containerName"], "example.shop",
            "{name}: {symbols}"
        );
        assert!(!dir.join(missing).exists());
        if missing_root {
            let manifest_uri = uri(&dir.join("crucible.toml"));
            let manifest = std::fs::read_to_string(dir.join("crucible.toml")).unwrap();
            client.open(&manifest_uri, &manifest);
            let diagnostics = client.diagnostics(&manifest_uri);
            assert!(
                !diagnostics.iter().any(|diagnostic| diagnostic["message"]
                    .as_str()
                    .unwrap()
                    .contains("not a directory")),
                "eligible open buffer: {diagnostics:#?}"
            );
            client.notify(
                "textDocument/didClose",
                json!({ "textDocument": { "uri": new_uri } }),
            );
            let diagnostics = client.diagnostics(&manifest_uri);
            assert!(
                diagnostics.iter().any(|diagnostic| diagnostic["message"]
                    .as_str()
                    .unwrap()
                    .contains("not a directory")),
                "closed buffer: {diagnostics:#?}"
            );
        }
        client.stop();
    }
}

#[test]
fn unsaved_files_outside_package_sources_stay_excluded() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-unsaved-excluded-source");
    package(&dir);
    let model = dir.join("model");
    std::fs::create_dir_all(&model).unwrap();
    std::fs::write(model.join("service.cru"), SERVICE).unwrap();
    std::fs::write(model.join("types.cru"), TYPES).unwrap();
    std::fs::write(
        dir.join("crucible.toml"),
        "[model]\nnamespace = \"example.shop\"\nsources = [\"model\"]\nimports = [\"crucible.errors\"]\n",
    )
    .unwrap();
    let outside_uri = uri(&dir.join("excluded.cru"));
    let mut client = Client::start();
    client.open(
        &outside_uri,
        "structure Excluded { value: DefinitelyMissing }\n",
    );
    assert!(client.diagnostics(&outside_uri).is_empty());
    let symbols = client.request("workspace/symbol", json!({ "query": "Excluded" }));
    assert_eq!(symbols, json!([]));
    client.stop();
}

#[test]
fn renaming_a_declaration_to_a_builtin_type_is_rejected() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-rename-builtin");
    package(&dir);
    let service_uri = uri(&dir.join("service.cru"));
    let mut client = Client::start();
    client.open(&service_uri, SERVICE);
    assert!(client.diagnostics(&service_uri).is_empty());
    for new_name in ["string", "integer", "document", "timestamp"] {
        client.send(json!({
            "jsonrpc": "2.0", "id": 900, "method": "textDocument/rename",
            "params": {
                "textDocument": { "uri": service_uri },
                "position": position(SERVICE, "ItemId", 0), "newName": new_name
            }
        }));
        let response = loop {
            let message = client.receive();
            if message["id"] == 900 {
                break message;
            }
        };
        assert!(response["error"].is_object(), "{new_name}: {response}");
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("built-in")
        );
    }
    client.stop();
}

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
    // The whole declaration, constraints included, in a fence an editor
    // highlights as Crucible, and the constraints again in words.
    assert!(
        text.starts_with("```crucible\nstring ItemId { length 1..10 }\n```"),
        "{text}"
    );
    assert!(text.contains("Constraints: 1 to 10 characters."), "{text}");
    assert_eq!(
        hover["range"]["start"],
        position(SERVICE, "ItemId", 0),
        "{hover}"
    );

    // A member: its type, whether it is required, and its type's constraints.
    let hover_text = |client: &mut Client, at: Value| {
        let hover = client.request(
            "textDocument/hover",
            json!({ "textDocument": { "uri": service }, "position": at }),
        );
        let text = hover["contents"]["value"]
            .as_str()
            .unwrap_or_else(|| panic!("no hover: {hover}"))
            .to_owned();
        assert_eq!(hover["contents"]["kind"], "markdown");
        // Every fence is tagged with the language and closed.
        let fences: Vec<&str> = text.lines().filter(|l| l.starts_with("```")).collect();
        assert!(fences.len() >= 2 && fences.len() % 2 == 0, "{text}");
        for pair in fences.chunks(2) {
            assert_eq!(pair, ["```crucible", "```"], "{text}");
        }
        text
    };
    let member = hover_text(&mut client, position(SERVICE, "id: ItemId", 0));
    assert!(
        member.starts_with("```crucible\nid: ItemId\n```"),
        "{member}"
    );
    assert!(
        member.contains("A required member of the input of `GetItem`."),
        "{member}"
    );
    assert!(
        member.contains("Constraints from `ItemId`: 1 to 10 characters."),
        "{member}"
    );

    // Keywords explain themselves, with an example, by where they are.
    let keyword = hover_text(&mut client, position(SERVICE, "readonly", 0));
    assert!(keyword.starts_with("**`readonly`**"), "{keyword}");
    assert!(keyword.contains("only reads"), "{keyword}");
    let clause = hover_text(&mut client, position(SERVICE, "protocol", 0));
    assert!(clause.contains("wire protocol"), "{clause}");
    let http = hover_text(&mut client, position(SERVICE, "http", 0));
    assert!(http.contains("HTTP binding"), "{http}");

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

#[test]
fn the_workspace_is_compiled_and_changes_on_disk_are_seen() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-workspace");
    package(&dir);
    let mut client = Client::start_with(json!({
        "rootUri": uri(&dir),
        "capabilities": { "workspace": { "didChangeWatchedFiles": { "dynamicRegistration": true } } },
    }));
    // The server asks to be told about model files and manifests.
    let registration = loop {
        let message = client.receive();
        if message["method"] == "client/registerCapability" {
            break message;
        }
    };
    let watchers = &registration["params"]["registrations"][0];
    assert_eq!(watchers["method"], "workspace/didChangeWatchedFiles");
    client.send(json!({ "jsonrpc": "2.0", "id": registration["id"], "result": null }));

    // Nothing is open, yet the package's declarations are found once the
    // background search has compiled it.
    let names = client.symbols_eventually("itemid", |names| names.contains(&"ItemId".to_owned()));
    assert!(names.contains(&"ItemId".to_owned()), "{names:?}");

    let service = uri(&dir.join("service.cru"));
    client.open(&service, SERVICE);
    assert_eq!(client.diagnostics(&service), Vec::<Value>::new());

    // Another tool renames the type in a file the editor does not have open.
    let types = dir.join("types.cru");
    std::fs::write(&types, TYPES.replace("ItemId", "ItemKey")).unwrap();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": uri(&types), "type": 2 }] }),
    );
    let diagnostics = client.diagnostics(&service);
    assert!(
        diagnostics
            .iter()
            .any(|d| d["message"].as_str().unwrap().contains("ItemId")),
        "{diagnostics:#?}"
    );
    std::fs::write(&types, TYPES).unwrap();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": uri(&types), "type": 2 }] }),
    );
    assert_eq!(client.diagnostics(&service), Vec::<Value>::new());
    client.stop();
}

const SHOP: &str = r#"/// A shop item. See https://example.com/items.
resource Item {
    id itemId: ItemId
    property size: Size
}

enum Size { SMALL, LARGE = "large" }

@since("1")
operation FindItem reads Item {
    http GET "/items/{itemId}"
    input { $itemId }
    output { size: Size }
}
"#;

#[test]
fn hints_lenses_links_and_signatures() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-insight");
    package(&dir);
    std::fs::write(dir.join("shop.cru"), SHOP).unwrap();
    let shop = uri(&dir.join("shop.cru"));
    // Usage lenses are for clients that run their command.
    assert!(Client::start().capabilities["codeLensProvider"].is_null());
    let mut client = Client::start_with(json!({
        "capabilities": {},
        "initializationOptions": { "showReferences": true },
    }));
    assert!(client.capabilities["codeLensProvider"].is_object());
    client.open(&shop, SHOP);
    client.diagnostics(&shop);
    let document = json!({ "uri": shop });

    // The elided member's type and the enum value's implicit wire value.
    let hints = client.request(
        "textDocument/inlayHint",
        json!({ "textDocument": document, "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 99, "character": 0 } } }),
    );
    let labels: Vec<&str> = hints
        .as_array()
        .unwrap()
        .iter()
        .map(|hint| hint["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["= \"SMALL\"", ": ItemId"], "{hints}");

    // A usage count above each declaration, which the editor shows on click.
    let lenses = client.request("textDocument/codeLens", json!({ "textDocument": document }));
    let titles: Vec<(&str, &str)> = lenses
        .as_array()
        .unwrap()
        .iter()
        .map(|lens| {
            (
                lens["command"]["title"].as_str().unwrap(),
                lens["command"]["command"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        titles,
        [
            ("1 usage", "crucible.showReferences"),
            ("2 usages", "crucible.showReferences"),
            ("no usages", "crucible.showReferences"),
        ],
        "{lenses}"
    );

    // From a member's name to its type, declared in another file.
    let target = client.request(
        "textDocument/typeDefinition",
        json!({ "textDocument": document, "position": position(SHOP, "itemId: ItemId", 0) }),
    );
    assert!(
        target["uri"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .ends_with("types.cru"),
        "{target}"
    );

    let links = client.request(
        "textDocument/documentLink",
        json!({ "textDocument": document }),
    );
    assert_eq!(links[0]["target"], "https://example.com/items", "{links}");

    let at = position(SHOP, "\"1\"", 0);
    let help = client.request(
        "textDocument/signatureHelp",
        json!({ "textDocument": document, "position": at }),
    );
    assert_eq!(
        help["signatures"][0]["label"], "@since(\"version\")",
        "{help}"
    );

    // Formatting a selection touches only its lines.
    let messy = SHOP
        .replace("    id itemId", "id itemId")
        .replace("    http GET", "http GET");
    client.change(&shop, 2, &messy);
    client.diagnostics(&shop);
    let line = messy[..messy.find("id itemId").unwrap()]
        .matches('\n')
        .count();
    let edits = client.request(
        "textDocument/rangeFormatting",
        json!({
            "textDocument": document,
            "range": { "start": { "line": line, "character": 0 }, "end": { "line": line, "character": 3 } },
            "options": { "tabSize": 4, "insertSpaces": true },
        }),
    );
    let edits = edits.as_array().unwrap();
    assert_eq!(edits.len(), 1, "{edits:?}");
    assert_eq!(edits[0]["newText"], "    id itemId: ItemId\n");
    assert_eq!(edits[0]["range"]["start"]["line"], line);
    assert_eq!(edits[0]["range"]["end"]["line"], line + 1);
    client.stop();
}

#[test]
fn the_manifest_policy_shows_in_model_files_and_edits_apply_at_once() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-policy");
    package(&dir);
    let policy = "[model]\nnamespace = \"example.shop\"\nimports = [\"crucible.errors\"]\n\n[rules.naming]\nmembers = { case = \"PascalCase\", severity = \"warning\" }\n";
    std::fs::write(dir.join("crucible.toml"), policy).unwrap();
    let service = uri(&dir.join("service.cru"));
    let mut client = Client::start();
    client.open(&service, SERVICE);

    // `id` and `name` are not PascalCase: warnings at the member names,
    // tagged with the rule.
    let diagnostics = client.diagnostics(&service);
    let policy_findings: Vec<&Value> = diagnostics
        .iter()
        .filter(|d| d["code"] == "naming.members")
        .collect();
    assert_eq!(policy_findings.len(), 2, "{diagnostics:#?}");
    assert!(policy_findings.iter().all(|d| d["severity"] == 2));
    let actions_at = |client: &mut Client, word: &str| {
        let finding = policy_findings
            .iter()
            .find(|d| d["range"]["start"] == position(SERVICE, word, 0))
            .unwrap();
        client.request(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": service },
                "range": finding["range"],
                "context": { "diagnostics": [finding] },
            }),
        )
    };
    // `name` is spelled once, so it is renamed in place.
    let actions = actions_at(&mut client, "name: string");
    assert_eq!(actions[0]["title"], "Rename to `Name`", "{actions}");
    // `id` is also the URI label `{id}`: renaming one would break the other.
    let actions = actions_at(&mut client, "id: ItemId");
    assert_eq!(actions, json!([]), "{actions}");

    // Editing the manifest, unsaved, changes what the model shows.
    let manifest = uri(&dir.join("crucible.toml"));
    client.open(&manifest, policy);
    client.diagnostics(&manifest);
    client.change(
        &manifest,
        2,
        "[model]\nnamespace = \"example.shop\"\nimports = [\"crucible.errors\"]\n\n[rules.naming]\nmembers = \"camelCase\"\n",
    );
    // Opening the manifest published once more; the edit's result follows.
    let mut cleared = false;
    for _ in 0..3 {
        cleared = client
            .diagnostics(&service)
            .iter()
            .all(|d| d["code"] != "naming.members");
        if cleared {
            break;
        }
    }
    assert!(cleared, "the camelCase members still had findings");

    // The manifest completes its own keys and values, and a bad value is
    // reported where it is written.
    let text = "[model]\nnamespace = \"example.shop\"\n\n[rules.naming]\nmembers = ";
    client.change(&manifest, 3, text);
    let items = client.request(
        "textDocument/completion",
        json!({ "textDocument": { "uri": manifest }, "position": { "line": 4, "character": 10 } }),
    );
    assert!(labels(&items).contains(&"camelCase".to_owned()), "{items}");
    client.change(
        &manifest,
        4,
        "[model]\nnamespace = \"example.shop\"\n\n[rules.naming]\nmembers = \"Camel\"\n",
    );
    let problems = loop {
        let problems = client.diagnostics(&manifest);
        if !problems.is_empty() {
            break problems;
        }
    };
    assert_eq!(problems[0]["range"]["start"]["line"], 4, "{problems:?}");
    client.stop();
}

#[test]
fn breaking_changes_against_the_baseline_show_as_you_type() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-baseline");
    package(&dir);
    // Freeze the package as it is, then point the manifest at the snapshot.
    let _ = std::fs::remove_dir_all(dir.join("releases"));
    let loaded = crucible_lang::load_package(&dir).unwrap();
    let service = crucible_lang::compile(&loaded, None).service.unwrap();
    crucible_lang::release::freeze(&dir.join("releases"), &service, "test").unwrap();
    std::fs::write(
        dir.join("crucible.toml"),
        format!(
            "[model]\nnamespace = \"example.shop\"\nimports = [\"crucible.errors\"]\n\n[model.baseline]\npath = \"releases/{}.json\"\n",
            service.version
        ),
    )
    .unwrap();
    let service = uri(&dir.join("service.cru"));
    let mut client = Client::start();
    client.open(&service, SERVICE);
    let unchanged = client.diagnostics(&service);
    assert!(
        unchanged
            .iter()
            .all(|d| !d["message"].as_str().unwrap().contains("baseline")),
        "{unchanged:#?}"
    );

    // Moving the route breaks old clients: shown at the operation's name.
    let moved = SERVICE.replace("\"/items/{id}\"", "\"/things/{id}\"");
    client.change(&service, 2, &moved);
    let diagnostics = client.diagnostics(&service);
    let breaking = diagnostics
        .iter()
        .find(|d| d["code"] == "compatibility.http")
        .unwrap_or_else(|| panic!("{diagnostics:#?}"));
    assert_eq!(breaking["severity"], 1);
    assert!(
        breaking["message"]
            .as_str()
            .unwrap()
            .starts_with("breaking change since 1:")
    );
    assert_eq!(breaking["range"]["start"], position(&moved, "GetItem {", 0));
    client.stop();
}

#[test]
fn nested_and_deleted_packages_stay_current() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-nested");
    let _ = std::fs::remove_dir_all(&root);
    let inner = root.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(
        root.join("crucible.toml"),
        "[model]\nnamespace = \"example.outer\"\n",
    )
    .unwrap();
    std::fs::write(root.join("a.cru"), "structure Outer {}\n").unwrap();
    std::fs::write(
        inner.join("crucible.toml"),
        "[model]\nnamespace = \"example.inner\"\n",
    )
    .unwrap();
    std::fs::write(inner.join("b.cru"), "structure Inner {}\n").unwrap();
    let mut client = Client::start_with(json!({
        "rootUri": uri(&root),
        "capabilities": { "workspace": { "didChangeWatchedFiles": { "dynamicRegistration": true } } },
    }));
    // A nested package is its own: the outer one does not compile its files.
    let inner_names = client.symbols_eventually("inner", |names| !names.is_empty());
    assert_eq!(inner_names, ["Inner"], "{inner_names:?}");

    // A file changes on disk: the old name goes.
    std::fs::write(inner.join("b.cru"), "structure Renamed {}\n").unwrap();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": uri(&inner.join("b.cru")), "type": 2 }] }),
    );
    let renamed = client.symbols_eventually("renamed", |names| !names.is_empty());
    assert_eq!(renamed, ["Renamed"]);
    assert!(client.symbols_eventually("inner", Vec::is_empty).is_empty());

    // The inner package's directory goes: so do its declarations, everywhere.
    std::fs::remove_dir_all(&inner).unwrap();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": uri(&inner), "type": 3 }] }),
    );
    let gone = client.symbols_eventually("renamed", Vec::is_empty);
    assert!(gone.is_empty(), "{gone:?}");
    assert_eq!(
        client.symbols_eventually("outer", |n| !n.is_empty()),
        ["Outer"]
    );
    client.stop();
}

/// The namespace each workspace symbol named `name` comes from.
fn containers(client: &mut Client, name: &str) -> Vec<String> {
    let symbols = client.request("workspace/symbol", json!({ "query": name }));
    let mut out: Vec<String> = symbols
        .as_array()
        .unwrap()
        .iter()
        .filter(|symbol| symbol["name"] == name)
        .map(|symbol| symbol["containerName"].as_str().unwrap().to_owned())
        .collect();
    out.sort();
    out
}

fn eventually(client: &mut Client, name: &str, want: &[&str]) -> Vec<String> {
    let mut got = Vec::new();
    for _ in 0..100 {
        got = containers(client, name);
        if got == want {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    got
}

#[test]
fn a_nested_manifest_changes_what_the_outer_package_compiles() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-nested-manifest");
    let _ = std::fs::remove_dir_all(&root);
    let inner = root.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(
        root.join("crucible.toml"),
        "[model]\nnamespace = \"example.outer\"\n",
    )
    .unwrap();
    std::fs::write(root.join("a.cru"), "structure Outer {}\n").unwrap();
    std::fs::write(inner.join("b.cru"), "structure Inner {}\n").unwrap();
    let mut client = Client::start_with(json!({
        "rootUri": uri(&root),
        "capabilities": { "workspace": { "didChangeWatchedFiles": { "dynamicRegistration": true } } },
    }));
    // Without its own manifest, `inner/` is part of the outer package.
    assert_eq!(
        eventually(&mut client, "Inner", &["example.outer"]),
        ["example.outer"]
    );

    // Its manifest appears: the outer package lets go of it.
    let manifest = inner.join("crucible.toml");
    std::fs::write(&manifest, "[model]\nnamespace = \"example.inner\"\n").unwrap();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": uri(&manifest), "type": 1 }] }),
    );
    assert_eq!(
        eventually(&mut client, "Inner", &["example.inner"]),
        ["example.inner"]
    );

    // And goes again: the outer package takes it back.
    std::fs::remove_file(&manifest).unwrap();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": uri(&manifest), "type": 3 }] }),
    );
    assert_eq!(
        eventually(&mut client, "Inner", &["example.outer"]),
        ["example.outer"]
    );
    client.stop();
}

#[test]
fn a_package_discovery_finds_is_one_whose_changes_are_seen() {
    // Discovery compares directory names as spelled, so it searches `Build`
    // on every file system; the watcher must not ignore it either.
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-cased");
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("Build").join("api");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("crucible.toml"),
        "[model]\nnamespace = \"example.cased\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("a.cru"), "structure First {}\n").unwrap();
    let mut client = Client::start_with(json!({
        "rootUri": uri(&root),
        "capabilities": { "workspace": { "didChangeWatchedFiles": { "dynamicRegistration": true } } },
    }));
    let first = client.symbols_eventually("first", |names| !names.is_empty());
    assert_eq!(first, ["First"]);
    std::fs::write(dir.join("a.cru"), "structure Second {}\n").unwrap();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": uri(&dir.join("a.cru")), "type": 2 }] }),
    );
    let second = client.symbols_eventually("second", |names| !names.is_empty());
    assert_eq!(second, ["Second"]);
    client.stop();
}

const MAIL_RESOURCES: &str = "resource Workspace {
    id workspaceId: string
    property name: string
}

resource Inbox in Workspace {
    id inboxId: string as id
    property title: string
}

resource Message in Inbox {
    id messageId: string as id
    property body: string
    readonly property sentAt: timestamp
}
";

#[test]
fn a_dollar_completes_what_the_member_list_may_bind() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-bindings");
    package(&dir);
    std::fs::write(dir.join("resources.cru"), MAIL_RESOURCES).unwrap();
    let text = "operation UpdateMessage updates Message {\n    input { $in }\n}\n";
    let file = dir.join("message.cru");
    std::fs::write(&file, text).unwrap();
    let file = uri(&file);
    let mut client = Client::start();
    let triggers = &client.capabilities["completionProvider"]["triggerCharacters"];
    assert!(
        triggers.as_array().unwrap().contains(&json!("$")),
        "{triggers}"
    );
    client.open(&file, text);
    client.diagnostics(&file);

    // After `$in`: the resource's identifiers, its ancestors' identifiers
    // and its own properties an `updates` input may write, from another file.
    let at = position(text, "$in", 0);
    let at = json!({ "line": at["line"], "character": at["character"].as_u64().unwrap() + 3 });
    let items = client.request(
        "textDocument/completion",
        json!({ "textDocument": { "uri": file }, "position": at }),
    );
    assert_eq!(
        labels(&items),
        ["workspaceId", "inboxId", "messageId", "body"],
        "{items}"
    );
    let inbox = &items[1];
    assert_eq!(inbox["detail"], "string — identifier of Inbox");
    // The `$` stays; `in` is replaced.
    assert_eq!(inbox["textEdit"]["newText"], "inboxId");
    assert_eq!(
        inbox["textEdit"]["range"]["start"]["character"].as_u64(),
        Some(at["character"].as_u64().unwrap() - 2)
    );
    assert_eq!(inbox["textEdit"]["range"]["end"], at);

    // A `$` typed where it binds nothing opens no list: in a comment, or at
    // the top level, where keywords would otherwise pop up.
    let typing = "// costs $\noperation UpdateMessage updates Message {\n    input { $in }\n}\n$";
    client.change(&file, 2, typing);
    client.diagnostics(&file);
    for (line, character) in [(0, 10), (4, 1)] {
        let items = client.request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": file },
                "position": { "line": line, "character": character },
                "context": { "triggerKind": 2, "triggerCharacter": "$" },
            }),
        );
        assert!(
            items.is_null() || items.as_array().is_some_and(Vec::is_empty),
            "{line}:{character}: {items}"
        );
    }
    client.stop();
}

const LINKED_SERVICE: &str = r#"service Shop {
    version "1"
    protocol json
    operations [GetItem]
}

/// Reads an [Item] by [GetItem.id]; see [Item.name], [Item][] and
/// [the item][Item]. Not `[Item]` in code.
readonly operation GetItem {
    http GET "/items/{id}"
    input { id: string }
    output { item?: Item }
}
"#;

const LINKED_TYPES: &str =
    "/// An item; [GetItem] returns it.\nstructure Item {\n    name: string\n}\n";

#[test]
fn doc_links_navigate_find_references_and_rename() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp-doc-links");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("crucible.toml"),
        "[model]\nnamespace = \"example.shop\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("service.cru"), LINKED_SERVICE).unwrap();
    std::fs::write(dir.join("types.cru"), LINKED_TYPES).unwrap();
    let service = uri(&dir.join("service.cru"));
    let types = uri(&dir.join("types.cru"));
    let mut client = Client::start();
    client.open(&service, LINKED_SERVICE);
    assert!(client.diagnostics(&service).is_empty());
    let at = |word: &str, nth: usize, by: usize| {
        let mut p = position(LINKED_SERVICE, word, nth);
        p["character"] = json!(p["character"].as_u64().unwrap() as usize + by);
        json!({ "textDocument": { "uri": service }, "position": p })
    };

    // `[Item]` (the first `Item` after `[`) goes to the structure in the
    // other file, `[Item.name]`'s member part to the member, and
    // `[GetItem.id]`'s to the input member.
    let definition = client.request("textDocument/definition", at("[Item]", 0, 1));
    assert_eq!(definition["uri"], types, "{definition}");
    assert_eq!(
        definition["range"]["start"],
        json!({ "line": 1, "character": 10 })
    );
    let definition = client.request("textDocument/definition", at("Item.name", 0, 5));
    assert_eq!(definition["uri"], types, "{definition}");
    assert_eq!(
        definition["range"]["start"],
        json!({ "line": 2, "character": 4 })
    );
    let definition = client.request("textDocument/definition", at("Item.name", 0, 1));
    assert_eq!(
        definition["range"]["start"],
        json!({ "line": 1, "character": 10 })
    );
    let definition = client.request("textDocument/definition", at("GetItem.id", 0, 8));
    assert_eq!(definition["uri"], service, "{definition}");
    assert_eq!(
        definition["range"]["start"],
        position(LINKED_SERVICE, "id: string", 0)
    );
    // A bracket in a code span is not a link.
    let definition = client.request("textDocument/definition", at("`[Item]`", 0, 3));
    assert!(definition.is_null(), "{definition}");

    // References from the declaration include every doc link to it, across
    // the package, and not the code span.
    client.open(&types, LINKED_TYPES);
    client.diagnostics(&types);
    let mut params = json!({
        "textDocument": { "uri": types },
        "position": position(LINKED_TYPES, "Item", 1),
        "context": { "includeDeclaration": false }
    });
    let references = client.request("textDocument/references", params.clone());
    let mut found: Vec<(String, u64, u64)> = references
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["uri"].as_str().unwrap().to_owned(),
                r["range"]["start"]["line"].as_u64().unwrap(),
                r["range"]["start"]["character"].as_u64().unwrap(),
            )
        })
        .collect();
    found.sort();
    let mut want: Vec<(String, u64, u64)> = [
        ("[Item]", 0),
        ("[Item.name]", 0),
        ("[Item][]", 0),
        ("[the item][Item]", 0),
        ("item?: Item", 0),
    ]
    .into_iter()
    .map(|(word, nth)| {
        let p = position(LINKED_SERVICE, word, nth);
        let skip = word.find("Item").unwrap() as u64;
        (
            service.clone(),
            p["line"].as_u64().unwrap(),
            p["character"].as_u64().unwrap() + skip,
        )
    })
    .collect();
    want.sort();
    assert_eq!(found, want);
    // From a doc link, the same references.
    params = at("[Item][]", 0, 1);
    params["context"] = json!({ "includeDeclaration": false });
    let from_link = client.request("textDocument/references", params);
    assert_eq!(
        from_link.as_array().unwrap().len(),
        want.len(),
        "{from_link}"
    );

    // Renaming rewrites the labels, the name part of `[Item.name]` too, and
    // leaves the code span alone.
    let prepared = client.request("textDocument/prepareRename", at("Item.name", 0, 2));
    assert_eq!(prepared["placeholder"], "Item", "{prepared}");
    let mut params = at("[the item][Item]", 0, 12);
    params["newName"] = json!("Thing");
    let edit = client.request("textDocument/rename", params);
    let apply = |text: &str, edits: &Value| {
        let mut text = text.to_owned();
        let mut edits: Vec<&Value> = edits.as_array().unwrap().iter().collect();
        edits.sort_by_key(|e| {
            (
                e["range"]["start"]["line"].as_u64().unwrap(),
                e["range"]["start"]["character"].as_u64().unwrap(),
            )
        });
        for e in edits.into_iter().rev() {
            let offset = |p: &Value| {
                let line = p["line"].as_u64().unwrap() as usize;
                let start: usize = text.split('\n').take(line).map(|l| l.len() + 1).sum();
                start + p["character"].as_u64().unwrap() as usize
            };
            let (start, end) = (offset(&e["range"]["start"]), offset(&e["range"]["end"]));
            text.replace_range(start..end, e["newText"].as_str().unwrap());
        }
        text
    };
    let renamed = apply(LINKED_SERVICE, &edit["changes"][&service]);
    assert!(
        renamed.contains("/// Reads an [Thing] by [GetItem.id]; see [Thing.name], [Thing][] and\n/// [the item][Thing]. Not `[Item]` in code."),
        "{renamed}"
    );
    assert!(renamed.contains("output { item?: Thing }"), "{renamed}");
    let renamed = apply(LINKED_TYPES, &edit["changes"][&types]);
    assert_eq!(
        renamed,
        "/// An item; [GetItem] returns it.\nstructure Thing {\n    name: string\n}\n"
    );
    // The displayed text of `[the item][Item]` navigates like its label.
    let definition = client.request("textDocument/definition", at("[the item][Item]", 0, 3));
    assert_eq!(definition["uri"], types, "{definition}");
    let mut params = at("[the item][Item]", 0, 3);
    params["context"] = json!({ "includeDeclaration": false });
    let from_text = client.request("textDocument/references", params);
    assert_eq!(
        from_text.as_array().unwrap().len(),
        want.len(),
        "{from_text}"
    );
    // A doc link names a capitalised declaration, so a lowercase new name
    // for a linked one is refused rather than turning its links into prose.
    let mut params = at("[Item][]", 0, 1);
    params["newName"] = json!("item");
    let refused = client.request_error("textDocument/rename", params);
    assert!(refused.contains("would be prose"), "{refused}");
    client.stop();
}
