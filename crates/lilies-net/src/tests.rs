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
fn test_decode_image_valid_png() {
    let mut img = ::image::RgbaImage::new(2, 2);
    img.put_pixel(0, 0, ::image::Rgba([255, 0, 0, 255]));
    img.put_pixel(1, 0, ::image::Rgba([0, 255, 0, 128]));
    img.put_pixel(0, 1, ::image::Rgba([0, 0, 255, 0]));
    img.put_pixel(1, 1, ::image::Rgba([255, 255, 255, 255]));

    let mut cursor = std::io::Cursor::new(Vec::new());
    img.write_to(&mut cursor, ::image::ImageFormat::Png).unwrap();
    let png_bytes = cursor.into_inner();

    let decoded = decode_image(&png_bytes).expect("decoding valid png bytes");
    assert_eq!(decoded.width, 2);
    assert_eq!(decoded.height, 2);
    assert_eq!(decoded.pixels.len(), 16);

    // Pixel (0, 0): Red opaque
    assert_eq!(&decoded.pixels[0..4], &[255, 0, 0, 255]);
    // Pixel (1, 0): Green 50% alpha
    assert_eq!(&decoded.pixels[4..8], &[0, 255, 0, 128]);
    // Pixel (0, 1): Blue fully transparent
    assert_eq!(&decoded.pixels[8..12], &[0, 0, 255, 0]);
    // Pixel (1, 1): White opaque
    assert_eq!(&decoded.pixels[12..16], &[255, 255, 255, 255]);
}

#[test]
fn test_decode_image_empty_and_corrupt_data() {
    let empty_err = decode_image(&[]).unwrap_err();
    assert!(matches!(empty_err, ImageError::EmptyData));

    let corrupt_err = decode_image(&[0x01, 0x02, 0x03, 0x04]).unwrap_err();
    assert!(matches!(corrupt_err, ImageError::Decode(_)));
}

#[test]
fn test_image_cache_insert_and_retrieve() {
    let cache = ImageCache::new();
    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);

    let img1 = std::sync::Arc::new(DecodedImage::new(10, 10, vec![0; 400]));
    cache.insert("https://example.com/logo.png", std::sync::Arc::clone(&img1));

    assert_eq!(cache.len(), 1);
    assert!(!cache.is_empty());
    assert!(cache.contains("https://example.com/logo.png"));
    assert!(!cache.contains("https://example.com/other.png"));

    let retrieved = cache.get("https://example.com/logo.png").expect("found in cache");
    assert_eq!(retrieved.width, 10);
    assert_eq!(retrieved.height, 10);

    let snapshot = cache.snapshot();
    assert_eq!(snapshot.len(), 1);
    assert!(snapshot.contains_key("https://example.com/logo.png"));
}

#[test]
fn test_fetch_bytes_local_file() {
    let temp_dir = std::env::temp_dir();
    let temp_file_path = temp_dir.join(format!("lilies_test_bytes_{}.bin", std::process::id()));
    let test_bytes = vec![0xDE, 0xAD, 0xBE, 0xEF];
    fs::write(&temp_file_path, &test_bytes).expect("write temp file");

    let path_str = temp_file_path.to_str().unwrap();
    let fetched = fetch_bytes(path_str).expect("fetch_bytes local file");
    assert_eq!(fetched, test_bytes);

    let _ = fs::remove_file(&temp_file_path);
}

#[test]
#[ignore]
fn test_live_fetch_example_com() {
    let html = fetch("https://example.com").expect("fetch example.com");
    assert!(html.contains("Example Domain"));
}
