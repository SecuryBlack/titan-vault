use std::{fs, process::Command};
fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_titanvault"))
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn cli_parallel_backup_catalog_recovery_and_failures() {
    let root = std::env::temp_dir().join(format!("titanvault-cli-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let input = root.join("input.txt");
    fs::write(&input, "parallel recovery payload").unwrap();
    let config = root.join("config.toml");
    let quote = |p: &std::path::Path| serde_json::to_string(&p.to_string_lossy()).unwrap();
    let text = format!(
        r#"
[schedule]
enabled = false
[crypto]
enabled = true
passphrase = "local test key"
[[sources.filesystems]]
name = "stack"
paths = [{}]
[targets.local]
enabled = true
path = {}
"#,
        quote(&input),
        quote(&root.join("store"))
    );
    fs::write(&config, &text).unwrap();
    let cfg = config.to_str().unwrap();
    let out = run(&["backup", "daily", "--config", cfg]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run(&["snapshots", "--target", "local", "--config", cfg]);
    assert!(out.status.success());
    let snapshots: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let path = snapshots[0]["path"].as_str().unwrap();
    assert!(path.starts_with("config/daily/"));
    let destination = root.join("recovered");
    let dest = destination.to_str().unwrap();
    let out = run(&[
        "restore",
        "--target",
        "local",
        "--snapshot",
        path,
        "--destination",
        dest,
        "--config",
        cfg,
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read_to_string(destination.join("input.txt")).unwrap(),
        "parallel recovery payload"
    );
    assert!(!run(&[
        "restore",
        "--target",
        "local",
        "--snapshot",
        path,
        "--destination",
        dest,
        "--config",
        cfg
    ])
    .status
    .success());
    assert!(run(&["prune", "--level", "daily", "--config", cfg])
        .status
        .success());
    assert!(!run(&["backup", "bad-level", "--config", cfg])
        .status
        .success());
    fs::write(&config, "[schedule]\nenabled = false\n").unwrap();
    assert!(!run(&["backup", "daily", "--config", cfg]).status.success());
    assert!(!run(&["test", "--config", cfg]).status.success());
    assert!(!run(&[
        "backup",
        "daily",
        "--config",
        root.join("absent.toml").to_str().unwrap()
    ])
    .status
    .success());
    fs::remove_dir_all(root).unwrap();
}
