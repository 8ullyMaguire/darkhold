//! M64-14 — the local Ollama provider speaks the real embeddings JSON shape.

use darkhold_app::embeddings_provider::OllamaEmbeddings;
use darkhold_domain::embeddings::{EmbeddingInput, EmbeddingProvider, EmbeddingSource};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn ollama_provider_reads_an_embedding_from_a_real_http_response() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock ollama");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut buffer = [0u8; 4096];
        let read = socket.read(&mut buffer).await.expect("read request");
        let request = String::from_utf8_lossy(&buffer[..read]);
        assert!(request.contains("/v1/embeddings"), "request: {request}");
        assert!(request.contains("local-model"), "request: {request}");
        let body = r#"{"data":[{"embedding":[0.25,0.5]}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("write response");
    });

    let provider = OllamaEmbeddings::new(
        format!("http://{address}/v1/embeddings"),
        "local-model".to_owned(),
    );
    assert!(provider.available());
    let input = EmbeddingInput {
        text: "a summary".to_owned(),
        source: EmbeddingSource::Summary,
    };
    let vector = provider.embed(&input).await.expect("embedding");
    assert_eq!(vector, vec![0.25, 0.5]);
    server.await.expect("server");
}

#[tokio::test]
async fn ollama_provider_is_unavailable_without_a_model() {
    let provider =
        OllamaEmbeddings::new("http://127.0.0.1:1/v1/embeddings".to_owned(), String::new());
    assert!(!provider.available());
}
