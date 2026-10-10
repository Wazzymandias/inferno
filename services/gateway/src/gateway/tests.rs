use std::{sync::Arc, time::Duration};

use tokio_util::sync::CancellationToken;

use axum::{
    Router,
    body::{Body, Bytes},
    http::{HeaderMap, Method, StatusCode, Uri, header},
    routing::{any, get},
};

use super::{
    Gateway,
    routes::{app, strip_transport_headers},
};
use crate::backend::{Backend, Pool};
use axum::http::HeaderValue;
use reqwest::{Client, Url};

use crate::backend::install_crypto_provider;
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_rustls::rustls;

async fn serve(router: Router) -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (format!("http://{address}"), task)
}

async fn serve_tls(version: &'static rustls::SupportedProtocolVersion) -> (u16, JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let certificate = include_bytes!("../testdata/localhost.der").to_vec().into();
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(
        include_bytes!("../testdata/localhost-key.der").to_vec(),
    );
    let config = rustls::ServerConfig::builder_with_protocol_versions(&[version])
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key.into())
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            // Invalid-certificate cases deliberately abort the TLS handshake.
            let Ok(mut stream) = acceptor.accept(socket).await else {
                continue;
            };
            let mut request = [0; 4096];
            if stream.read(&mut request).await.unwrap() == 0 {
                continue;
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
            stream.shutdown().await.unwrap();
        }
    });
    (port, task)
}

#[tokio::test]
async fn https_verifies_certificates_and_hostnames_with_tls12_and_tls13() {
    // Construct the production backend first to initialize its crypto provider.
    let backend = Backend::new(
        Url::parse("https://localhost/v1").unwrap(),
        Duration::from_secs(5),
        None,
    )
    .unwrap();
    let ca = reqwest::Certificate::from_der(include_bytes!("../testdata/ca.der")).unwrap();
    let trusted = Client::builder()
        .tls_certs_only([ca])
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    for version in [&rustls::version::TLS12, &rustls::version::TLS13] {
        let (port, task) = serve_tls(version).await;
        let response = trusted
            .get(format!("https://localhost:{port}/v1/models"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "ok");

        let untrusted = backend
            .client
            .get(format!("https://localhost:{port}/v1/models"))
            .send()
            .await
            .unwrap_err();
        assert!(format!("{untrusted:?}").contains("UnknownIssuer"));

        let wrong_hostname = trusted
            .get(format!("https://127.0.0.1:{port}/v1/models"))
            .send()
            .await
            .unwrap_err();
        assert!(format!("{wrong_hostname:?}").contains("NotValidForName"));
        task.abort();
    }
}

#[tokio::test]
async fn preserves_backend_prefix_query_body_and_error_response() {
    let mock = Router::new().route(
        "/engines/v1/chat/completions",
        any(
            |method: Method, uri: Uri, headers: HeaderMap, body: Bytes| async move {
                assert_eq!(method, Method::POST);
                assert_eq!(uri.query(), Some("trace=1"));
                assert_eq!(headers[header::AUTHORIZATION], "Bearer test");
                assert!(!headers.contains_key("x-hop"));
                assert_eq!(body, r#"{"model":"test","messages":[]}"#);
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    [
                        ("x-backend", "preserved"),
                        ("content-type", "application/json"),
                    ],
                    r#"{"error":"backend validation"}"#,
                )
            },
        ),
    );
    let (upstream, mock_task) = serve(mock).await;
    let mut pool = Pool::new();
    pool.add(
        Url::parse(&format!("{upstream}/engines/v1")).unwrap(),
        Duration::from_secs(5),
        Some("Bearer default-credential".parse().unwrap()),
    )
    .unwrap();
    let (gateway, api_task) = serve(app(
        Arc::new(Gateway::new("127.0.0.1".parse().unwrap(), 0, processor()).with_pool(pool)),
        1024,
    ))
    .await;
    let response = Client::new()
        .post(format!("{gateway}/v1/chat/completions?trace=1"))
        .header(header::AUTHORIZATION, "Bearer test")
        .header(header::CONNECTION, "x-hop")
        .header("x-hop", "remove me")
        .body(r#"{"model":"test","messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.headers()["x-backend"], "preserved");
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert_eq!(
        response.text().await.unwrap(),
        r#"{"error":"backend validation"}"#
    );
    api_task.abort();
    mock_task.abort();
}

#[tokio::test]
async fn streams_first_event_before_backend_finishes() {
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(2);
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    let body = Body::from_stream(stream);
    let body = std::sync::Arc::new(tokio::sync::Mutex::new(Some(body)));
    let mock = Router::new().route(
        "/v1/chat/completions",
        get(move || {
            let body = std::sync::Arc::clone(&body);
            async move {
                (
                    [(header::CONTENT_TYPE, "text/event-stream")],
                    body.lock().await.take().unwrap(),
                )
            }
        }),
    );
    let (upstream, mock_task) = serve(mock).await;
    let mut pool = Pool::new();
    pool.add(
        Url::parse(&format!("{upstream}/v1")).unwrap(),
        Duration::from_secs(5),
        None,
    )
    .unwrap();
    let (gateway, api_task) = serve(app(
        Arc::new(Gateway::new("127.0.0.1".parse().unwrap(), 0, processor()).with_pool(pool)),
        1024,
    ))
    .await;
    tx.send(Ok(Bytes::from_static(b"data: first\n\n")))
        .await
        .unwrap();
    let mut response = Client::new()
        .get(format!("{gateway}/v1/chat/completions"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );
    let first = tokio::time::timeout(Duration::from_secs(1), response.chunk())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(first, "data: first\n\n");
    tx.send(Ok(Bytes::from_static(b"data: [DONE]\n\n")))
        .await
        .unwrap();
    drop(tx);
    assert_eq!(response.chunk().await.unwrap().unwrap(), "data: [DONE]\n\n");
    assert!(response.chunk().await.unwrap().is_none());
    api_task.abort();
    mock_task.abort();
}

#[tokio::test]
async fn times_out_an_unfinished_backend_stream() {
    let mock = Router::new().route(
        "/v1/chat/completions",
        get(|| async {
            let stream = futures_util::stream::unfold(true, |first| async move {
                if first {
                    Some((
                        Ok::<_, std::io::Error>(Bytes::from_static(b"data: first\n\n")),
                        false,
                    ))
                } else {
                    std::future::pending().await
                }
            });
            Body::from_stream(stream)
        }),
    );
    let (upstream, mock_task) = serve(mock).await;
    let mut pool = Pool::new();
    pool.add(
        Url::parse(&format!("{upstream}/v1")).unwrap(),
        Duration::from_secs(1),
        None,
    )
    .unwrap();
    let (gateway, api_task) = serve(app(
        Arc::new(Gateway::new("127.0.0.1".parse().unwrap(), 0, processor()).with_pool(pool)),
        1024,
    ))
    .await;
    let mut response = Client::new()
        .get(format!("{gateway}/v1/chat/completions"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.chunk().await.unwrap().unwrap(), "data: first\n\n");
    let result = tokio::time::timeout(Duration::from_secs(3), response.chunk())
        .await
        .expect("gateway must terminate the stream when its backend timeout expires");
    assert!(
        result.is_err(),
        "an incomplete stream must end with an error"
    );
    api_task.abort();
    mock_task.abort();
}

#[tokio::test]
async fn distinguishes_liveness_backend_failure_and_oversized_request() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let mut pool = Pool::new();
    pool.add(
        Url::parse(&format!("http://{address}/v1")).unwrap(),
        Duration::from_secs(1),
        None,
    )
    .unwrap();
    let (gateway, task) = serve(app(
        Arc::new(Gateway::new("127.0.0.1".parse().unwrap(), 0, processor()).with_pool(pool)),
        4,
    ))
    .await;
    let client = Client::new();
    for (path, expected) in [
        ("/healthz", StatusCode::OK),
        ("/readyz", StatusCode::SERVICE_UNAVAILABLE),
        ("/v1/models", StatusCode::BAD_GATEWAY),
    ] {
        let response = client.get(format!("{gateway}{path}")).send().await.unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::BAD_GATEWAY {
            assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
            assert_eq!(
                response.text().await.unwrap(),
                r#"{"error":{"message":"inference backend unavailable"}}"#
            );
        }
    }
    assert_eq!(
        client
            .post(format!("{gateway}/v1/chat/completions"))
            .body("too large")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE,
    );
    task.abort();
}

#[test]
fn removes_all_connection_nominated_headers() {
    let mut headers = HeaderMap::new();
    headers.append(header::CONNECTION, HeaderValue::from_static("x-one"));
    headers.append(
        header::CONNECTION,
        HeaderValue::from_static("X-Two, keep-alive"),
    );
    headers.insert("x-one", HeaderValue::from_static("one"));
    headers.insert("x-two", HeaderValue::from_static("two"));
    strip_transport_headers(&mut headers);
    assert!(headers.is_empty());
}

#[tokio::test]
async fn shutdown_deadline_bounds_an_active_request() {
    install_crypto_provider();
    let client = Client::new();
    let (started_tx, mut started_rx) = mpsc::channel(1);
    let router = Router::new().route(
        "/hang",
        get(move || {
            let started_tx = started_tx.clone();
            async move {
                started_tx.send(()).await.unwrap();
                std::future::pending::<StatusCode>().await
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (upstream, mock_task) = serve(router).await;
    let mut pool = Pool::new();
    pool.add(Url::parse(&upstream).unwrap(), Duration::from_secs(5), None)
        .unwrap();
    let mut gateway = Gateway::new(address.ip(), address.port(), processor()).with_pool(pool);
    gateway.shutdown_timeout = Duration::from_millis(50);
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(Arc::new(gateway).serve_http(listener, shutdown.clone()));
    let request =
        tokio::spawn(async move { client.get(format!("http://{address}/v1/hang")).send().await });
    tokio::time::timeout(Duration::from_secs(2), started_rx.recv())
        .await
        .unwrap()
        .unwrap();
    shutdown.cancel();
    let result = tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
    request.abort();
    mock_task.abort();
}

#[tokio::test]
async fn idle_server_shuts_down_cleanly() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut gateway = Gateway::new(address.ip(), address.port(), processor());
    gateway.shutdown_timeout = Duration::from_secs(1);
    let shutdown = CancellationToken::new();
    shutdown.cancel();
    tokio::time::timeout(
        Duration::from_secs(2),
        Arc::new(gateway).serve_http(listener, shutdown),
    )
    .await
    .unwrap()
    .unwrap();
}

fn processor() -> crate::inference::InputProcessor {
    crate::inference::InputProcessor::load(
        crate::inference::ModelConfig::parse(
            include_bytes!("../testdata/input-string/model-config.json"),
            "fixture",
        )
        .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn streaming_load_lasts_until_completion_cancellation_or_body_error() {
    let mock = Router::new().fallback(any(|uri: Uri| async move {
        if uri.query() != Some("stream") {
            return Body::from("done");
        }
        Body::from_stream(futures_util::stream::unfold(true, |first| async move {
            if first {
                Some((
                    Ok::<_, std::io::Error>(Bytes::from_static(b"data: first\n\n")),
                    false,
                ))
            } else {
                std::future::pending().await
            }
        }))
    }));
    let (upstream, task) = serve(mock).await;
    let mut pool = Pool::new();
    for replica in ["zero", "one"] {
        pool.add(
            format!("{upstream}/{replica}/v1").parse().unwrap(),
            Duration::from_secs(1),
            None,
        )
        .unwrap();
    }
    let gateway = Gateway::new("127.0.0.1".parse().unwrap(), 0, processor()).with_pool(pool);
    let request = || {
        serde_json::from_value(
            serde_json::json!({"model":"fixture", "input":"hello", "stream":true}),
        )
        .unwrap()
    };
    let input = gateway.processor.prepare(&request()).unwrap();
    let selected = || {
        gateway
            .select(&input)
            .unwrap()
            .0
            .url("responses", None)
            .path()
            .to_owned()
    };
    let response = gateway
        .create_response(Some("stream"), HeaderMap::new(), request())
        .await
        .unwrap();
    assert_eq!(selected(), "/one/v1/responses");
    drop(response);
    assert_eq!(selected(), "/zero/v1/responses");
    let response = gateway
        .create_response(None, HeaderMap::new(), request())
        .await
        .unwrap();
    assert_eq!(selected(), "/one/v1/responses");
    axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(selected(), "/zero/v1/responses");
    let response = gateway
        .create_response(Some("stream"), HeaderMap::new(), request())
        .await
        .unwrap();
    assert!(
        axum::body::to_bytes(response.into_body(), 1024)
            .await
            .is_err()
    );
    assert_eq!(selected(), "/zero/v1/responses");
    task.abort();
}

#[tokio::test]
async fn prepares_locally_and_forwards_once_without_mutating_the_request() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let mock = Router::new().fallback(any(move |uri: Uri, headers: HeaderMap, body: Bytes| {
        let observed = Arc::clone(&observed);
        async move {
            assert_eq!(
                uri.path(),
                "/v1/responses",
                "preparation must not make render calls"
            );
            assert_eq!(headers[header::CONTENT_LENGTH], body.len().to_string());
            observed.fetch_add(1, Ordering::SeqCst);
            (
                [(header::CONTENT_TYPE, headers[header::CONTENT_TYPE].clone())],
                body,
            )
        }
    }));
    let (upstream, mock_task) = serve(mock).await;
    let mut pool = Pool::new();
    pool.add(
        Url::parse(&format!("{upstream}/v1")).unwrap(),
        Duration::from_secs(5),
        None,
    )
    .unwrap();
    let (gateway, task) = serve(app(
        Arc::new(Gateway::new("127.0.0.1".parse().unwrap(), 0, processor()).with_pool(pool)),
        4096,
    ))
    .await;
    let request = serde_json::json!({"model":"fixture", "input":"Keep the original", "instructions":"System", "stream":true});
    // Serialization removes whitespace and omitted nullable fields, so the
    // outbound Content-Length must describe the encoded body, not the original.
    let raw_request = r#"{ "model": "fixture", "input": "Keep the original", "instructions": "System", "stream": true, "metadata": null }"#;
    for (connection, content_type) in [
        ("keep-alive", "application/json; charset=utf-8"),
        ("content-type", "application/json"),
    ] {
        let response = Client::new()
            .post(format!("{gateway}/v1/responses"))
            .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
            .header(header::CONNECTION, connection)
            .body(raw_request)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], content_type);
        assert_eq!(response.json::<serde_json::Value>().await.unwrap(), request);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let invalid = Client::new()
        .post(format!("{gateway}/v1/responses"))
        .json(&serde_json::json!({"model":"other", "input":"private"}))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert!(!invalid.text().await.unwrap().contains("private"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    task.abort();
    mock_task.abort();
}

#[tokio::test]
async fn outbound_json_and_http_build_failures_share_a_safe_server_error() {
    use crate::inference::InputError;
    use axum::response::IntoResponse;
    use std::error::Error;

    let serialization = InputError::from(<serde_json::Error as serde::ser::Error>::custom(
        "private request data",
    ));
    assert!(serialization.source().unwrap().is::<serde_json::Error>());
    let construction = InputError::from(
        Client::new()
            .post("http://localhost")
            .header("x-test", "private request data\n")
            .build()
            .unwrap_err(),
    );
    assert!(construction.source().unwrap().is::<reqwest::Error>());

    for error in [serialization, construction] {
        assert!(!format!("{error:?}").contains("private request data"));
        assert!(!error.to_string().contains("private request data"));
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["error"]["type"], "server_error");
        assert_eq!(
            body["error"]["message"],
            "The gateway could not build the backend request."
        );
    }
}

#[tokio::test]
async fn task_failure_preserves_its_typed_cause_in_the_input_error() {
    use crate::inference::InputError;
    use axum::response::IntoResponse;
    use std::error::Error;
    let task = tokio::spawn(std::future::pending::<()>());
    task.abort();
    let error = InputError::from(task.await.unwrap_err());
    let cause = error
        .source()
        .unwrap()
        .downcast_ref::<tokio::task::JoinError>()
        .unwrap();
    assert!(cause.is_cancelled());
    let error = InputError::PreparationFailed(Box::new(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "private request data",
    )));
    assert_eq!(
        error
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap()
            .kind(),
        std::io::ErrorKind::BrokenPipe
    );
    assert!(!format!("{error:?}").contains("private request data"));
    assert!(!error.to_string().contains("private request data"));
    let response = error.into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["error"]["type"], "server_error");
}
