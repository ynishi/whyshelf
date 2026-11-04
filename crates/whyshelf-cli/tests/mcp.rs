//! `whyshelf mcp` over its standard input and output, as a client sees it.

mod common;

use std::process::Stdio;

use common::{Repo, finding_text};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};

/// A client speaking newline-delimited JSON-RPC to the server.
struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    async fn start(repo: &Repo) -> Client {
        let mut child = repo
            .command()
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the server can be started");
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
        let mut client = Client {
            child,
            stdin,
            stdout,
            next_id: 0,
        };
        let init = client
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "test-client", "version": "0.0.0" }
                }),
            )
            .await;
        assert!(
            init["result"]["capabilities"]["tools"].is_object(),
            "{init}"
        );
        client
            .send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
        client
    }

    async fn send(&mut self, message: &Value) {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .expect("the server reads");
        self.stdin.flush().await.expect("the server reads");
    }

    /// Send a request and read until its answer comes (notifications are skipped).
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        loop {
            let mut line = String::new();
            let read = self
                .stdout
                .read_line(&mut line)
                .await
                .expect("the server answers");
            assert!(read > 0, "the server closed its output before answering");
            let message: Value = serde_json::from_str(&line).expect("a JSON line");
            if message["id"] == json!(id) {
                return message;
            }
        }
    }

    /// Call a tool; returns whether it was an error, and its text.
    async fn call(&mut self, tool: &str, arguments: Value) -> (bool, String) {
        let answer = self
            .request(
                "tools/call",
                json!({ "name": tool, "arguments": arguments }),
            )
            .await;
        let result = &answer["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text in {answer}"))
            .to_owned();
        (result["isError"].as_bool().unwrap_or(false), text)
    }

    async fn finish(mut self) {
        drop(self.stdin);
        let status = self.child.wait().await.expect("the server ends");
        assert!(status.success(), "the server ends cleanly: {status:?}");
    }
}

#[tokio::test]
async fn the_tools_are_the_commands_and_every_answer_opens_with_the_context() {
    let repo = Repo::new().await;
    repo.init_project().await;
    let mut client = Client::start(&repo).await;

    let listed = client.request("tools/list", json!({})).await;
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for name in [
        "whyshelf_context",
        "whyshelf_add",
        "whyshelf_tag",
        "whyshelf_mv",
        "whyshelf_publish",
        "whyshelf_finding_new",
        "whyshelf_spec_new",
        "whyshelf_design_new",
        "whyshelf_check",
        "whyshelf_revoke",
        "whyshelf_get",
        "whyshelf_find",
        "whyshelf_status",
        "whyshelf_trace",
        "whyshelf_changelog",
        "whyshelf_audit",
        "whyshelf_rebuild",
    ] {
        assert!(names.contains(&name), "{name} in {names:?}");
    }
    // The tools that list take limit, cursor and fields.
    for tool in listed["result"]["tools"].as_array().unwrap() {
        let name = tool["name"].as_str().unwrap();
        if ["find", "status", "trace", "changelog", "audit"]
            .iter()
            .any(|n| name == format!("whyshelf_{n}"))
        {
            let properties = &tool["inputSchema"]["properties"];
            for property in ["limit", "cursor", "fields"] {
                assert!(properties[property].is_object(), "{name} has {property}");
            }
        }
    }

    // A tool and its command make the same change; the first line is the context.
    let (error, text) = client
        .call(
            "whyshelf_finding_new",
            json!({ "finding_type": "usage_observation", "title": "Login feels slow", "format": "json" }),
        )
        .await;
    assert!(!error, "{text}");
    let (head, body) = text.split_once('\n').unwrap();
    assert!(
        head.starts_with("[context: project=demo, worktree="),
        "{head}"
    );
    let created: Value = serde_json::from_str(body).unwrap();
    assert_eq!(created["created"]["alias"], "finding/login-feels-slow");
    let path = created["created"]["path"].as_str().unwrap();
    repo.write(
        path,
        &finding_text("Login feels slow", "It takes eight seconds."),
    )
    .await;

    let (error, text) = client
        .call(
            "whyshelf_check",
            json!({ "gate": "cr", "target": "finding/login-feels-slow", "approve": true }),
        )
        .await;
    assert!(!error, "{text}");
    assert!(text.contains("CR at"), "{text}");

    // The same change is visible to the command line.
    let status = repo.json(&["status"]).await;
    assert_eq!(status["entities"][0]["approval"], "Approved");

    // A list is cut to the window asked for, and says where to continue.
    repo.write(
        "docs/finding/other.md",
        &finding_text("Other", "Something else."),
    )
    .await;
    let (error, text) = client
        .call(
            "whyshelf_add",
            json!({ "paths": ["docs/finding/other.md"] }),
        )
        .await;
    assert!(!error, "{text}");
    let (_, text) = client
        .call(
            "whyshelf_status",
            json!({ "limit": 1, "fields": ["id", "approval"], "format": "json" }),
        )
        .await;
    let (_, body) = text.split_once('\n').unwrap();
    let page: Value = serde_json::from_str(body).unwrap();
    assert_eq!(page["entities"].as_array().unwrap().len(), 1);
    assert_eq!(page["entities"][0].as_object().unwrap().len(), 2);
    let cursor = page["next_cursor"].as_str().unwrap().to_owned();
    let (_, text) = client
        .call(
            "whyshelf_status",
            json!({ "limit": 1, "cursor": cursor, "format": "json" }),
        )
        .await;
    let (_, body) = text.split_once('\n').unwrap();
    let page: Value = serde_json::from_str(body).unwrap();
    assert_eq!(page["entities"].as_array().unwrap().len(), 1);
    assert!(page["next_cursor"].is_null());

    // A failure is an error answer, and it too opens with the context.
    let (error, text) = client
        .call("whyshelf_get", json!({ "target": "finding/nothing-here" }))
        .await;
    assert!(error);
    assert!(text.starts_with("[context: project=demo"), "{text}");
    assert!(text.contains("names nothing"), "{text}");
    let (error, text) = client
        .call(
            "whyshelf_tag",
            json!({ "target": "finding/other", "changes": ["area:auth"] }),
        )
        .await;
    assert!(error);
    assert!(text.starts_with("[context: project=demo"), "{text}");

    client.finish().await;
}
