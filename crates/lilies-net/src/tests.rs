#![deny(unsafe_code)]

use super::*;
use std::fs;

#[test]
fn test_empty_url_returns_invalid_url_error() {
    let err = fetch("   ").unwrap_err();
    assert!(matches!(err, NetError::InvalidUrl(_)));
}

#[test]
fn test_unsupported_schemes_return_unsupported_scheme_error() {
    let unsupported = [
        "ftp://example.com",
        "gopher://example.com",
        "ws://echo.websocket.events",
        "custom://foo",
    ];
    for url in unsupported {
        let err = fetch(url).unwrap_err();
        match err {
            NetError::UnsupportedScheme(scheme) => {
                assert!(url.starts_with(&scheme));
            }
            other => panic!("Expected UnsupportedScheme, got {:?}", other),
        }
    }
}

#[test]
fn test_fetch_file_scheme_and_local_path() {
    let temp_dir = std::env::temp_dir();
    let temp_file_path = temp_dir.join(format!("lilies_test_{}.html", std::process::id()));
    let content = "<h1>Hello from local file</h1>";
    fs::write(&temp_file_path, content).expect("write temp file");

    let path_str = temp_file_path.to_str().unwrap();

    // 1. Direct path
    let res = fetch(path_str).expect("fetch direct path");
    assert_eq!(res, content);

    // 2. file:// scheme
    let file_url = format!("file://{}", path_str);
    let res_file = fetch(&file_url).expect("fetch file://");
    assert_eq!(res_file, content);

    // Clean up
    let _ = fs::remove_file(&temp_file_path);

    // 3. Non-existent file
    let err = fetch("file:///non_existent_path_12345.html").unwrap_err();
    assert!(matches!(err, NetError::Io(_)));
}

#[test]
fn test_user_agent_and_redirects_constants() {
    assert_eq!(USER_AGENT, "lilies/0.1.0");
    assert_eq!(MAX_REDIRECTS, 5);
}

#[test]
#[ignore]
fn test_live_fetch_example_com() {
    let html = fetch("https://example.com").expect("fetch example.com");
    assert!(html.contains("Example Domain"));
}
