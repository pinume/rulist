mod common;
use common::{app_for, json_request, login_token};

use axum::http::StatusCode;
use rulist::db;
use rulist::permissions::{COPY, MOVE, WRITE_CONTENT};
use serde_json::json;

#[tokio::test]
async fn file_responses_keep_signed_fields_out_of_filesystem_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(root.join("folder")).unwrap();
    std::fs::write(root.join("hello.txt"), "hello").unwrap();
    let pool = db::init_db(&temp.path().join("rulist.db")).await.unwrap();
    for username in ["first-user", "second-user"] {
        db::create_user(
            &pool,
            username,
            "FilesPass123!",
            0,
            Some(root.to_str().unwrap()),
            0,
            false,
        )
        .await
        .unwrap();
    }
    let app = app_for(&pool).await;
    let first_token = login_token(&app, "first-user", "FilesPass123!").await;
    let second_token = login_token(&app, "second-user", "FilesPass123!").await;

    let (status, listing) = json_request(
        &app,
        "POST",
        "/api/fs/list",
        Some(&first_token),
        json!({ "path": "/" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let entries = listing["data"]["content"].as_array().unwrap();
    let file = entries
        .iter()
        .find(|entry| entry["name"] == "hello.txt")
        .unwrap();
    assert!(!file["sign"].as_str().unwrap().is_empty());
    assert!(
        file["raw_url"]
            .as_str()
            .unwrap()
            .starts_with("/p/hello.txt?sign=")
    );
    let folder = entries
        .iter()
        .find(|entry| entry["name"] == "folder")
        .unwrap();
    assert_eq!(folder["sign"], "");
    assert_eq!(folder["raw_url"], "");

    let (_, first_file) = json_request(
        &app,
        "POST",
        "/api/fs/get",
        Some(&first_token),
        json!({ "path": "/hello.txt" }),
    )
    .await;
    let (_, second_file) = json_request(
        &app,
        "POST",
        "/api/fs/get",
        Some(&second_token),
        json!({ "path": "/hello.txt" }),
    )
    .await;
    assert_ne!(first_file["data"]["sign"], second_file["data"]["sign"]);
    assert!(first_file["data"]["raw_url"].is_string());
}

#[tokio::test]
async fn document_preview_preserves_content_and_limits() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    std::fs::create_dir(&root).unwrap();
    for (name, content) in [
        ("readme.md", "# Hello\n<script>alert(1)</script>\n"),
        ("data.json", "{\"value\":1}"),
        ("invalid.json", "not json"),
        ("plain.txt", "中文\ntext\n"),
    ] {
        std::fs::write(root.join(name), content).unwrap();
    }
    std::fs::write(root.join("binary.txt"), [0xff]).unwrap();
    std::fs::write(root.join("limit.txt"), vec![b'x'; 4 * 1024 * 1024]).unwrap();
    std::fs::write(root.join("large.txt"), vec![b'x'; 4 * 1024 * 1024 + 1]).unwrap();
    let pool = db::init_db(&temp.path().join("rulist.db")).await.unwrap();
    db::create_user(
        &pool,
        "preview-user",
        "PreviewPass123!",
        0,
        Some(root.to_str().unwrap()),
        0,
        false,
    )
    .await
    .unwrap();
    let app = app_for(&pool).await;
    let token = login_token(&app, "preview-user", "PreviewPass123!").await;
    for (name, expected) in [
        ("readme.md", "<h1>Hello</h1>\n\n"),
        ("data.json", "{\n  \"value\": 1\n}"),
        ("invalid.json", "not json"),
        ("plain.txt", "中文\ntext\n"),
    ] {
        let (status, response) = json_request(
            &app,
            "POST",
            "/api/fs/preview",
            Some(&token),
            json!({"path": format!("/{name}")}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["data"]["content"]["value"], expected);
    }
    for (name, error) in [
        ("binary.txt", "unsupported_encoding"),
        ("large.txt", "too_large"),
    ] {
        let (_, response) = json_request(
            &app,
            "POST",
            "/api/fs/preview",
            Some(&token),
            json!({"path": format!("/{name}")}),
        )
        .await;
        assert_eq!(response["data"]["error"], error);
    }
    let (_, response) = json_request(
        &app,
        "POST",
        "/api/fs/preview",
        Some(&token),
        json!({"path": "/limit.txt"}),
    )
    .await;
    assert_eq!(
        response["data"]["content"]["value"].as_str().unwrap().len(),
        4 * 1024 * 1024
    );
}

#[tokio::test]
async fn filesystem_handlers_map_typed_errors_and_conflicts_to_http_statuses() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    let copy_src = root.join("copy-src");
    let copy_dst = root.join("copy-dst");
    let move_src = root.join("move-src");
    let move_dst = root.join("move-dst");
    for dir in [&copy_src, &copy_dst, &move_src, &move_dst] {
        tokio::fs::create_dir_all(dir).await.unwrap();
    }
    tokio::fs::write(root.join("plain-file"), b"file")
        .await
        .unwrap();
    tokio::fs::write(copy_src.join("exists.txt"), b"source copy")
        .await
        .unwrap();
    tokio::fs::write(copy_dst.join("exists.txt"), b"destination copy")
        .await
        .unwrap();
    tokio::fs::write(move_src.join("exists.txt"), b"source move")
        .await
        .unwrap();
    tokio::fs::write(move_dst.join("exists.txt"), b"destination move")
        .await
        .unwrap();
    std::os::unix::fs::symlink("missing-target", copy_dst.join("broken-link")).unwrap();

    let pool = db::init_db(&temp.path().join("rulist.db")).await.unwrap();
    db::create_user(
        &pool,
        "file-user",
        "FilesPass123!",
        0,
        Some(root.to_str().unwrap()),
        (1 << WRITE_CONTENT) | (1 << MOVE) | (1 << COPY),
        false,
    )
    .await
    .unwrap();
    let app = app_for(&pool).await;
    let token = login_token(&app, "file-user", "FilesPass123!").await;

    let (status, missing) = json_request(
        &app,
        "POST",
        "/api/fs/get",
        Some(&token),
        json!({ "path": "/missing.txt" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["code"], 404);

    let (status, missing_preview) = json_request(
        &app,
        "POST",
        "/api/fs/preview",
        Some(&token),
        json!({ "path": "/missing.txt" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing_preview["code"], 404);

    let (status, invalid_path) = json_request(
        &app,
        "POST",
        "/api/fs/list",
        Some(&token),
        json!({ "path": "/../plain-file" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid_path["code"], 400);

    let (status, forbidden_symlink) = json_request(
        &app,
        "POST",
        "/api/fs/get",
        Some(&token),
        json!({ "path": "/copy-dst/broken-link" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(forbidden_symlink["code"], 403);

    let (status, not_directory) = json_request(
        &app,
        "POST",
        "/api/fs/list",
        Some(&token),
        json!({ "path": "/plain-file/child" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(not_directory["code"], 400);

    let (status, empty_mkdir) = json_request(
        &app,
        "POST",
        "/api/fs/mkdir",
        Some(&token),
        json!({ "path": "" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(empty_mkdir["code"], 400);

    for (route, src_dir, dst_dir) in [
        ("/api/fs/copy", "/copy-src", "/copy-dst"),
        ("/api/fs/move", "/move-src", "/move-dst"),
    ] {
        let (status, conflict) = json_request(
            &app,
            "POST",
            route,
            Some(&token),
            json!({
                "src_dir": src_dir,
                "dst_dir": dst_dir,
                "names": ["exists.txt"],
                "conflict_policy": "cancel",
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{route}: {conflict}");
        assert_eq!(conflict["code"], 409);
    }

    let (status, duplicate_names) = json_request(
        &app,
        "POST",
        "/api/fs/copy",
        Some(&token),
        json!({
            "src_dir": "/copy-src",
            "dst_dir": "/copy-dst",
            "names": ["exists.txt", "exists.txt"],
            "conflict_policy": "skip",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(duplicate_names["code"], 400);

    let (status, symlink_conflict) = json_request(
        &app,
        "POST",
        "/api/fs/copy",
        Some(&token),
        json!({
            "src_dir": "/copy-src",
            "dst_dir": "/copy-dst",
            "names": ["broken-link"],
            "conflict_policy": "cancel",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(symlink_conflict["code"], 409);
}

#[tokio::test]
async fn upload_stream_failures_and_conflicts_preserve_existing_data() {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    struct BrokenInput(bool);
    impl tokio::io::AsyncRead for BrokenInput {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if self.0 {
                std::task::Poll::Ready(Err(std::io::Error::other("input interrupted")))
            } else {
                self.0 = true;
                buf.put_slice(b"partial");
                std::task::Poll::Ready(Ok(()))
            }
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("original.txt"), b"original").unwrap();
    let pool = db::init_db(&temp.path().join("data.db")).await.unwrap();
    db::create_user(
        &pool,
        "upload-user",
        "UploadPass123!",
        0,
        Some(root.to_str().unwrap()),
        (1 << WRITE_CONTENT) | (1 << rulist::permissions::OVERWRITE),
        false,
    )
    .await
    .unwrap();
    let app = app_for(&pool).await;
    let token = login_token(&app, "upload-user", "UploadPass123!").await;
    // Prime the listing cache before a write.
    json_request(
        &app,
        "POST",
        "/api/fs/list",
        Some(&token),
        json!({"path": "/"}),
    )
    .await;
    let request = |path: &str, overwrite: bool, body: Body| {
        Request::builder()
            .method("PUT")
            .uri("/api/fs/put")
            .header("Authorization", format!("Bearer {token}"))
            .header("File-Path", path)
            .header("Overwrite", overwrite.to_string())
            .body(body)
            .unwrap()
    };
    let failed = app
        .clone()
        .oneshot(request(
            "/original.txt",
            true,
            Body::from_stream(tokio_util::io::ReaderStream::new(BrokenInput(false))),
        ))
        .await
        .unwrap();
    assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        std::fs::read(root.join("original.txt")).unwrap(),
        b"original"
    );
    let (first, second) = tokio::join!(
        app.clone()
            .oneshot(request("/race.txt", false, Body::from("first"))),
        app.clone()
            .oneshot(request("/race.txt", false, Body::from("second"))),
    );
    let statuses = [first.unwrap().status(), second.unwrap().status()];
    assert_eq!(statuses.iter().filter(|&&s| s == StatusCode::OK).count(), 1);
    assert_eq!(
        statuses
            .iter()
            .filter(|&&s| s == StatusCode::CONFLICT)
            .count(),
        1
    );
    let content = std::fs::read(root.join("race.txt")).unwrap();
    assert!(content == b"first" || content == b"second");
    let (_, listing) = json_request(
        &app,
        "POST",
        "/api/fs/list",
        Some(&token),
        json!({"path": "/"}),
    )
    .await;
    assert_eq!(listing["data"]["total"], 2);
    assert!(std::fs::read_dir(&root).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".rulist-upload-")
    }));
}

#[tokio::test]
async fn batch_failure_reports_completed_items_and_preserves_conflicts() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("files");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir(root.join("dst")).unwrap();
    std::fs::write(root.join("src/a.txt"), b"a").unwrap();
    std::fs::write(root.join("src/b.txt"), b"source").unwrap();
    std::fs::write(root.join("dst/b.txt"), b"destination").unwrap();
    let pool = db::init_db(&temp.path().join("data.db")).await.unwrap();
    db::create_user(
        &pool,
        "batch-user",
        "BatchPass123!",
        0,
        Some(root.to_str().unwrap()),
        (1 << COPY) | (1 << MOVE) | (1 << rulist::permissions::DELETE),
        false,
    )
    .await
    .unwrap();
    let app = app_for(&pool).await;
    let token = login_token(&app, "batch-user", "BatchPass123!").await;
    let (status, _) = json_request(&app, "POST", "/api/fs/copy", Some(&token),
        json!({"src_dir":"/src", "dst_dir":"/dst", "names":["a.txt","b.txt"], "conflict_policy":"cancel"})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(!root.join("dst/a.txt").exists());
    let (status, _) = json_request(&app, "POST", "/api/fs/copy", Some(&token),
        json!({"src_dir":"/src", "dst_dir":"/dst", "names":["a.txt","b.txt"], "conflict_policy":"skip"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        std::fs::read(root.join("dst/b.txt")).unwrap(),
        b"destination"
    );
    let (status, error) = json_request(&app, "POST", "/api/fs/move", Some(&token),
        json!({"src_dir":"/src", "dst_dir":"/dst", "names":["b.txt","missing.txt"], "conflict_policy":"skip"})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("0 item(s) already moved")
    );
    let (status, error) = json_request(
        &app,
        "POST",
        "/api/fs/remove",
        Some(&token),
        json!({"dir":"/dst", "names":["a.txt","missing.txt"]}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("1 item(s) already deleted")
    );
    assert!(!root.join("dst/a.txt").exists());
}
