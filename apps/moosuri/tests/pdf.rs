use std::{
    io::Write,
    process::{Command, Stdio},
};

fn document(text: &str) -> Vec<u8> {
    let content = format!("BT /F1 12 Tf 10 20 Td ({text}) Tj ET");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
    ];
    let mut bytes = b"%PDF-1.5\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer\n<< /Root 1 0 R /Size 6 >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}

#[test]
fn extractor_cli_does_not_open_installation_and_returns_only_fixed_protocol() {
    let root = tempfile::tempdir().unwrap();
    let absent = root.path().join("must-not-be-created");
    let mut child = Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .args(["--data-dir", absent.to_str().unwrap(), "internal-pdf-text"])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&document("Synthetic receipt 37"))
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(!absent.exists());
    let value: Result<moosuri_documents::PdfText, moosuri_documents::PdfError> =
        serde_json::from_slice(&output.stdout).unwrap();
    let value = value.unwrap();
    assert_eq!(value.pages, 1);
    assert!(value.text.contains("Synthetic receipt 37"));
}

#[tokio::test]
async fn actual_resource_limited_child_extracts_and_rejects_no_text_without_another_runtime() {
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_moosuri"));
    let value = moosuri_documents::extract_pdf(executable, &document("Synthetic amount 37"))
        .await
        .unwrap();
    assert!(value.text.contains("[Page 1]\nSynthetic amount 37"));
    assert!(matches!(
        moosuri_documents::extract_pdf(executable, &document("")).await,
        Err(moosuri_documents::PdfError::NoText)
    ));
    assert!(matches!(
        moosuri_documents::extract_pdf(executable, b"%PDF-1.5\ntruncated").await,
        Err(moosuri_documents::PdfError::Invalid)
    ));
}

async fn request(
    address: std::net::SocketAddr,
    login: &moosuri_store::LoginResult,
    method: &str,
    path: &str,
    body: &[u8],
) -> (u16, String, Vec<u8>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let header = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nCookie: moosuri_session={}\r\nX-Moosuri-Account: {}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        login.token,
        login.user.account().id,
        body.len()
    );
    socket.write_all(header.as_bytes()).await.unwrap();
    socket.write_all(body).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(15),
        socket.take(3 * 1024 * 1024).read_to_end(&mut bytes),
    )
    .await
    .unwrap()
    .unwrap();
    let split = bytes.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
    let headers = String::from_utf8(bytes[..split].to_vec()).unwrap();
    assert!(
        !headers
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
    );
    (
        headers.split_whitespace().nth(1).unwrap().parse().unwrap(),
        headers,
        bytes[split + 4..].to_vec(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_http_upload_uses_child_and_preserves_original_retry_owner_and_delete() {
    let root = tempfile::tempdir().unwrap();
    let store = moosuri_store::Store::open(root.path()).unwrap();
    let password = "Synthetic-PDF-HTTP-password-2026";
    store
        .bootstrap(
            "Synthetic".into(),
            "pdf@example.test".into(),
            password.into(),
            false,
        )
        .await
        .unwrap();
    let admin = store
        .login("pdf@example.test".into(), password.into())
        .await
        .unwrap();
    let invite = store
        .create_user(
            &admin.user,
            "Synthetic owner".into(),
            "pdf-owner@example.test".into(),
        )
        .await
        .unwrap();
    let restricted = store
        .login("pdf-owner@example.test".into(), invite.temporary_password)
        .await
        .unwrap();
    let owner = store
        .change_password(&restricted.user, "Synthetic-PDF-owner-password-2026".into())
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let config = moosuri_server::ServerConfig::new(&format!("http://{address}"), true)
        .unwrap()
        .with_pdf_extractor(std::path::PathBuf::from(env!("CARGO_BIN_EXE_moosuri")))
        .unwrap();
    let runtime = config.start_runtime(store.clone());
    let router = moosuri_server::router_with_runtime(store.clone(), config, runtime.clone());
    let serve = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let id = "51515151-5151-4151-8151-515151515151";
    let path = format!("/api/v1/files/{id}?filename=receipt.pdf");
    let bytes = document("Synthetic total 37");
    let uploaded = request(address, &owner, "POST", &path, &bytes).await;
    assert_eq!(uploaded.0, 200);
    let receipt: serde_json::Value = serde_json::from_slice(&uploaded.2).unwrap();
    assert_eq!(receipt["media_type"], "application/pdf");
    assert_eq!(receipt["state"], "ready");
    let retry = request(address, &owner, "POST", &path, &bytes).await;
    assert_eq!(retry.0, 200);
    assert_eq!(retry.2, uploaded.2);
    let preview = request(
        address,
        &owner,
        "GET",
        &format!("/api/v1/files/{id}/preview"),
        &[],
    )
    .await;
    assert_eq!(preview.0, 200);
    assert!(
        preview
            .1
            .to_ascii_lowercase()
            .contains("cache-control: no-store")
    );
    assert!(
        String::from_utf8(preview.2)
            .unwrap()
            .contains("Synthetic total 37")
    );
    let original = request(
        address,
        &owner,
        "GET",
        &format!("/api/v1/files/{id}/content"),
        &[],
    )
    .await;
    assert_eq!(original.0, 200);
    assert_eq!(original.2, bytes);
    assert!(original.1.contains("document.pdf"));
    assert_eq!(
        request(
            address,
            &admin,
            "GET",
            &format!("/api/v1/files/{id}/content"),
            &[]
        )
        .await
        .0,
        404
    );
    let no_text = request(
        address,
        &owner,
        "POST",
        "/api/v1/files/52525252-5252-4252-8252-525252525252?filename=scan.pdf",
        &document(""),
    )
    .await;
    assert_eq!(no_text.0, 400);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&no_text.2).unwrap()["code"],
        "pdf_no_text"
    );
    assert_eq!(
        request(
            address,
            &owner,
            "DELETE",
            &format!("/api/v1/media/{id}"),
            &[]
        )
        .await
        .0,
        204
    );
    assert_eq!(
        request(
            address,
            &owner,
            "GET",
            &format!("/api/v1/files/{id}/preview"),
            &[]
        )
        .await
        .0,
        404
    );
    assert_eq!(request(address, &owner, "POST", &path, &bytes).await.0, 409);
    serve.abort();
    let _ = serve.await;
    runtime.shutdown().await;
}
