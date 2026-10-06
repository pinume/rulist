use rulist::filesystem::local::LocalFs;
use std::os::unix::ffi::OsStringExt;

fn driver_with_hidden(root: &std::path::Path, show_hidden: bool) -> LocalFs {
    LocalFs::new(root, show_hidden).unwrap()
}

fn driver(root: &std::path::Path) -> LocalFs {
    driver_with_hidden(root, false)
}

#[tokio::test]
async fn rejects_traversal_and_filesystem_root_removal() {
    let temp = tempfile::tempdir().unwrap();
    let driver = driver(temp.path());

    assert!(driver.get("../etc/passwd").await.is_err());
    assert!(driver.get("folder/../../etc").await.is_err());
    assert!(driver.remove("").await.is_err());
    assert!(driver.remove("/").await.is_err());
    assert!(temp.path().exists());
}

#[tokio::test]
async fn rejects_symlink_escape() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
    let driver = driver(root.path());

    assert!(driver.get("escape/secret.txt").await.is_err());
    assert!(driver.open("escape/secret.txt").await.is_err());
}

#[tokio::test]
async fn rejects_special_files() {
    let temp = tempfile::tempdir().unwrap();
    let socket_path = temp.path().join("test.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
    let fs = driver(temp.path());

    let entries = fs.list("").await.unwrap();
    assert!(!entries.iter().any(|entry| entry.name == "test.sock"));
    assert!(fs.get("test.sock").await.is_err());
    assert!(fs.open("test.sock").await.is_err());
}

#[tokio::test]
async fn skips_non_utf8_filenames() {
    let temp = tempfile::tempdir().unwrap();
    let invalid_name = std::ffi::OsString::from_vec(b"invalid-\xff.txt".to_vec());
    std::fs::write(temp.path().join(invalid_name), b"hidden from web").unwrap();
    tokio::fs::write(temp.path().join("valid.txt"), b"visible")
        .await
        .unwrap();
    let fs = driver(temp.path());

    let entries = fs.list("").await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "valid.txt");
}

#[tokio::test]
async fn hidden_paths_follow_show_hidden_policy() {
    let temp = tempfile::tempdir().unwrap();
    tokio::fs::create_dir_all(temp.path().join(".secret"))
        .await
        .unwrap();
    tokio::fs::write(temp.path().join(".secret/file.txt"), b"secret")
        .await
        .unwrap();
    tokio::fs::write(temp.path().join("visible.txt"), b"visible")
        .await
        .unwrap();

    let hidden = driver_with_hidden(temp.path(), false);
    assert!(hidden.get(".secret/file.txt").await.is_err());
    assert!(hidden.list(".secret").await.is_err());
    assert!(hidden.mkdir(".created").await.is_err());
    assert!(
        hidden
            .rename_safe("visible.txt", ".renamed", false)
            .await
            .is_err()
    );
    assert!(
        hidden
            .batch_rename("", &[("visible.txt".to_string(), ".batch".to_string())])
            .await
            .is_err()
    );

    let visible = driver_with_hidden(temp.path(), true);
    assert!(visible.get(".secret/file.txt").await.is_ok());
    let entries = visible.list(".secret").await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "file.txt");
}

#[tokio::test]
async fn copy_and_move_preserve_expected_contents() {
    let temp = tempfile::tempdir().unwrap();
    let driver = driver(temp.path());

    tokio::fs::create_dir_all(temp.path().join("source/nested"))
        .await
        .unwrap();
    tokio::fs::write(temp.path().join("source/nested/file.txt"), b"content")
        .await
        .unwrap();

    driver.copy_to("source", "copy").await.unwrap();
    assert_eq!(
        tokio::fs::read(temp.path().join("copy/nested/file.txt"))
            .await
            .unwrap(),
        b"content"
    );

    driver
        .move_to("copy/nested/file.txt", "moved/file.txt")
        .await
        .unwrap();
    assert!(!temp.path().join("copy/nested/file.txt").exists());
    assert_eq!(
        tokio::fs::read(temp.path().join("moved/file.txt"))
            .await
            .unwrap(),
        b"content"
    );
}

#[tokio::test]
async fn cancelling_copy_request_does_not_interrupt_staged_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    tokio::fs::create_dir(&source).await.unwrap();
    for index in 0..512 {
        tokio::fs::write(source.join(format!("{index:04}.txt")), b"copy")
            .await
            .unwrap();
    }
    let fs = driver(temp.path());
    let request_fs = fs.clone();
    let request =
        tokio::spawn(async move { request_fs.copy_to_safe("source", "copy", false).await });

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let mut entries = tokio::fs::read_dir(temp.path()).await.unwrap();
            let mut stage_exists = false;
            while let Some(entry) = entries.next_entry().await.unwrap() {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".rulist-copy-stage-")
                {
                    stage_exists = true;
                    break;
                }
            }
            if stage_exists {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Ok(mut entries) = tokio::fs::read_dir(temp.path().join("copy")).await {
                let mut count = 0;
                while entries.next_entry().await.unwrap().is_some() {
                    count += 1;
                }
                if count == 512 {
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    assert!(temp.path().join("copy/0511.txt").exists());
    let mut entries = tokio::fs::read_dir(temp.path()).await.unwrap();
    let mut root_entries = Vec::new();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        root_entries.push(entry.file_name().to_string_lossy().into_owned());
    }
    assert_eq!(root_entries.len(), 2);
    assert!(
        !root_entries
            .iter()
            .any(|name| name.starts_with(".rulist-copy-stage-"))
    );
}

#[tokio::test]
async fn cancelling_batch_rename_request_finishes_the_staged_rename() {
    let temp = tempfile::tempdir().unwrap();
    let mut pairs = Vec::new();
    for index in 0..128 {
        let src = format!("{index:04}.txt");
        let dst = format!("renamed-{index:04}.txt");
        tokio::fs::write(temp.path().join(&src), b"rename")
            .await
            .unwrap();
        pairs.push((src, dst));
    }
    let fs = driver(temp.path());
    let request_fs = fs.clone();
    let request_pairs = pairs.clone();
    let request = tokio::spawn(async move { request_fs.batch_rename("", &request_pairs).await });

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let mut entries = tokio::fs::read_dir(temp.path()).await.unwrap();
            let mut stage_exists = false;
            while let Some(entry) = entries.next_entry().await.unwrap() {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".rulist-rename-stage-")
                {
                    stage_exists = true;
                    break;
                }
            }
            if stage_exists {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if pairs.iter().all(|(_, dst)| temp.path().join(dst).exists()) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(pairs.iter().all(|(src, _)| !temp.path().join(src).exists()));
}

#[tokio::test]
async fn overwrite_is_explicit_for_copy_and_rename() {
    let temp = tempfile::tempdir().unwrap();
    let driver = driver(temp.path());

    tokio::fs::write(temp.path().join("source.txt"), b"new")
        .await
        .unwrap();
    tokio::fs::write(temp.path().join("target.txt"), b"old")
        .await
        .unwrap();

    assert!(
        driver
            .copy_to_safe("source.txt", "target.txt", false)
            .await
            .is_err()
    );
    assert_eq!(
        tokio::fs::read(temp.path().join("target.txt"))
            .await
            .unwrap(),
        b"old"
    );

    driver
        .copy_to_safe("source.txt", "target.txt", true)
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(temp.path().join("target.txt"))
            .await
            .unwrap(),
        b"new"
    );

    driver
        .copy_to_safe("source.txt", "copy-no-overwrite.txt", false)
        .await
        .unwrap();
    driver
        .move_to_safe("copy-no-overwrite.txt", "move-no-overwrite.txt", false)
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(temp.path().join("move-no-overwrite.txt"))
            .await
            .unwrap(),
        b"new"
    );

    tokio::fs::write(temp.path().join("move-source.txt"), b"source remains")
        .await
        .unwrap();
    tokio::fs::write(temp.path().join("move-target.txt"), b"existing target")
        .await
        .unwrap();
    assert!(
        driver
            .move_to_safe("move-source.txt", "move-target.txt", false)
            .await
            .is_err()
    );
    assert_eq!(
        tokio::fs::read(temp.path().join("move-source.txt"))
            .await
            .unwrap(),
        b"source remains"
    );
    assert_eq!(
        tokio::fs::read(temp.path().join("move-target.txt"))
            .await
            .unwrap(),
        b"existing target"
    );

    tokio::fs::write(temp.path().join("rename-source.txt"), b"renamed")
        .await
        .unwrap();
    assert!(
        driver
            .rename_safe("rename-source.txt", "target.txt", false)
            .await
            .is_err()
    );
    driver
        .rename_safe("rename-source.txt", "target.txt", true)
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(temp.path().join("target.txt"))
            .await
            .unwrap(),
        b"renamed"
    );

    tokio::fs::write(
        temp.path().join("rename-no-overwrite.txt"),
        b"renamed safely",
    )
    .await
    .unwrap();
    driver
        .rename_safe("rename-no-overwrite.txt", "renamed-no-overwrite.txt", false)
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(temp.path().join("renamed-no-overwrite.txt"))
            .await
            .unwrap(),
        b"renamed safely"
    );
}
