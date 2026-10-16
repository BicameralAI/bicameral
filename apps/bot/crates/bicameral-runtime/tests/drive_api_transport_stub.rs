//! Deterministic provider-backed tests for the production Google Drive
//! REST transport (#1086 residual): a std-only HTTP stub stands in for
//! Google so the full adapter append/readback path, credential refresh
//! rotation, marker-bound discovery/replay queries, pagination, and typed
//! failure mapping are exercised without live credentials. Live Google
//! behavior is explicitly not claimed by these tests.

use bicameral_config::{GoogleOAuthConfig, InMemorySecretsStore, SecretsStore};
use bicameral_runtime::drive_api_event_store::{
    DriveApiAdapter, DriveApiTransport, DriveTransportError, DriveTransportErrorKind,
};
use bicameral_runtime::drive_api_transport::{
    production_drive_event_store, DriveAccessTokenProvider, HttpDriveApiTransport,
    KeyringGoogleAccessTokenProvider, CANONICAL_EVENT_MARKER_KEY,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// ── Minimal deterministic HTTP stub (std only) ────────────────────

#[derive(Debug, Clone)]
struct RecordedRequest {
    method: String,
    path_and_query: String,
    content_type: Option<String>,
    body: Vec<u8>,
}

type Handler = Box<dyn Fn(&RecordedRequest) -> (u16, Vec<u8>) + Send + Sync>;

struct StubServer {
    base_url: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    running: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl StubServer {
    fn start(handler: Handler) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let running = Arc::new(AtomicBool::new(true));

        let thread_requests = requests.clone();
        let thread_running = running.clone();
        let thread = std::thread::spawn(move || {
            while thread_running.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let request = read_request(&mut stream);
                        let (status, body) = handler(&request);
                        thread_requests.lock().unwrap().push(request);
                        let response = format!(
                            "HTTP/1.1 {status} STUB\r\nContent-Type: application/json\r\n\
                             Content-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.write_all(&body);
                    }
                    Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            base_url,
            requests,
            running,
            thread: Some(thread),
        }
    }

    fn recorded(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for StubServer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_request(stream: &mut std::net::TcpStream) -> RecordedRequest {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let header_end = buffer
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(buffer.len());
    let headers = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let request_line = headers.lines().next().unwrap_or_default().to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path_and_query = parts.next().unwrap_or_default().to_string();

    let content_length = headers
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("content-length:"))
        .and_then(|line| line.split(':').nth(1))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let content_type = headers
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("content-type:"))
        .and_then(|line| line.split_once(':'))
        .map(|(_, value)| value.trim().to_string());

    let mut body = buffer[header_end..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    RecordedRequest {
        method,
        path_and_query,
        content_type,
        body,
    }
}

fn percent_decode(value: &str) -> String {
    let mut out = String::new();
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                out.push(byte as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

// ── Fixtures ──────────────────────────────────────────────────────

fn store_with_envelope(
    expires_in: Option<u64>,
    refresh: Option<&str>,
) -> Arc<InMemorySecretsStore> {
    let store = Arc::new(InMemorySecretsStore::new());
    let envelope = bicameral_config::OAuthTokenEnvelopeV1::from_token_exchange(
        "test-access-token-value".into(),
        refresh.map(String::from),
        expires_in,
        Some("Bearer".into()),
        None,
        chrono::Utc::now(),
    );
    store
        .write_secret(
            bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
            bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            &envelope.to_secret_string().unwrap(),
        )
        .unwrap();
    store
}

fn oauth_config() -> GoogleOAuthConfig {
    GoogleOAuthConfig {
        client_id: "test-client-id".into(),
        client_secret: "test-client-secret".into(),
        redirect_uri: "http://localhost:3000/cb".into(),
        scopes: vec!["https://www.googleapis.com/auth/drive.file".into()],
    }
}

struct StaticTokenProvider(String);
impl DriveAccessTokenProvider for StaticTokenProvider {
    fn access_token(&self) -> Result<String, DriveTransportError> {
        Ok(self.0.clone())
    }
}

const EVENT_ID: &str = "00000000-0000-4000-8000-000000000001";

fn event() -> bicameral_api::event_store::EventStoreEntry {
    let mut entry = bicameral_api::event_store::EventStoreEntry::new(
        bicameral_api::event_store::EventType::CreateProduct,
        serde_json::json!({"product_id": "prd-1"}),
        "test".into(),
    );
    entry.id = uuid::Uuid::parse_str(EVENT_ID).unwrap();
    entry
}

// ── Token provider tests (#1084 composition) ──────────────────────

#[test]
fn unexpired_envelope_access_token_is_returned_without_refresh() {
    let provider = KeyringGoogleAccessTokenProvider::new(
        store_with_envelope(Some(3600), Some("test-refresh-token")),
        Some(oauth_config()),
    );
    assert_eq!(provider.access_token().unwrap(), "test-access-token-value");
}

#[test]
fn expired_envelope_refreshes_rotates_and_persists() {
    let server = StubServer::start(Box::new(|request| {
        assert_eq!(request.method, "POST");
        let body = String::from_utf8_lossy(&request.body).to_string();
        assert!(body.contains("grant_type=refresh_token"));
        assert!(body.contains("refresh_token=test-refresh-token"));
        (
            200,
            serde_json::json!({
                "access_token": "rotated-access-token",
                "expires_in": 3599,
                "token_type": "Bearer"
            })
            .to_string()
            .into_bytes(),
        )
    }));

    let store = store_with_envelope(Some(0), Some("test-refresh-token"));
    let provider = KeyringGoogleAccessTokenProvider::new(store.clone(), Some(oauth_config()))
        .with_token_endpoint(format!("{}/token", server.base_url));

    assert_eq!(provider.access_token().unwrap(), "rotated-access-token");

    // Rotated envelope was re-persisted canonically, preserving the
    // refresh token Google omitted from the refresh response.
    let stored = store
        .read_secret(
            bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
            bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
        )
        .unwrap();
    let parsed = bicameral_config::parse_stored_oauth_token(&stored).unwrap();
    assert_eq!(parsed.access_token(), Some("rotated-access-token"));
    assert_eq!(parsed.refresh_token(), Some("test-refresh-token"));
}

#[test]
fn token_provider_fails_closed() {
    // No credential at all.
    let provider = KeyringGoogleAccessTokenProvider::new(
        Arc::new(InMemorySecretsStore::new()),
        Some(oauth_config()),
    );
    assert_eq!(
        provider.access_token().unwrap_err().kind,
        DriveTransportErrorKind::Authentication
    );

    // Expired without a refresh token.
    let provider = KeyringGoogleAccessTokenProvider::new(
        store_with_envelope(Some(0), None),
        Some(oauth_config()),
    );
    assert_eq!(
        provider.access_token().unwrap_err().kind,
        DriveTransportErrorKind::Authentication
    );

    // Refresh rejected by the provider; message stays secret-safe.
    let server = StubServer::start(Box::new(|_| (400, b"{}".to_vec())));
    let provider = KeyringGoogleAccessTokenProvider::new(
        store_with_envelope(Some(0), Some("test-refresh-token")),
        Some(oauth_config()),
    )
    .with_token_endpoint(format!("{}/token", server.base_url));
    let error = provider.access_token().unwrap_err();
    assert_eq!(error.kind, DriveTransportErrorKind::Authentication);
    assert!(!error.safe_message.contains("test-refresh-token"));
}

#[test]
fn legacy_plain_token_passes_through() {
    let store = Arc::new(InMemorySecretsStore::new());
    store
        .write_secret(
            bicameral_config::connections::legacy_aliases::GOOGLE_DRIVE_SOURCE_AUTH,
            bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            "legacy-plain-access-token",
        )
        .unwrap();
    let provider = KeyringGoogleAccessTokenProvider::new(store, None);
    assert_eq!(
        provider.access_token().unwrap(),
        "legacy-plain-access-token"
    );
}

/// #1134: a token expiring inside the deterministic skew window refreshes
/// proactively instead of departing near-dead.
#[test]
fn near_expiry_token_refreshes_within_skew() {
    let server = StubServer::start(Box::new(|_| {
        (
            200,
            serde_json::json!({
                "access_token": "skew-rotated-access-token",
                "expires_in": 3599,
                "token_type": "Bearer"
            })
            .to_string()
            .into_bytes(),
        )
    }));

    // 30s remaining < 60s skew → refresh.
    let provider = KeyringGoogleAccessTokenProvider::new(
        store_with_envelope(Some(30), Some("test-refresh-token")),
        Some(oauth_config()),
    )
    .with_token_endpoint(format!("{}/token", server.base_url));
    assert_eq!(
        provider.access_token().unwrap(),
        "skew-rotated-access-token"
    );
    assert_eq!(server.recorded().len(), 1);
}

/// #1134: native/public refresh never sends an empty pseudo-secret; a
/// confidential legacy config still includes its real secret.
#[test]
fn native_refresh_omits_client_secret_confidential_includes_it() {
    let server = StubServer::start(Box::new(|_| {
        (
            200,
            serde_json::json!({
                "access_token": "rotated-access-token",
                "expires_in": 3599,
                "token_type": "Bearer"
            })
            .to_string()
            .into_bytes(),
        )
    }));

    let native = GoogleOAuthConfig {
        client_id: "native-client-id".into(),
        client_secret: String::new(),
        redirect_uri: "http://localhost:3000/cb".into(),
        scopes: vec!["https://www.googleapis.com/auth/drive.file".into()],
    };
    let provider = KeyringGoogleAccessTokenProvider::new(
        store_with_envelope(Some(0), Some("test-refresh-token")),
        Some(native),
    )
    .with_token_endpoint(format!("{}/token", server.base_url));
    provider.access_token().unwrap();
    let native_body = String::from_utf8_lossy(&server.recorded()[0].body).to_string();
    assert!(!native_body.contains("client_secret"));
    assert!(native_body.contains("client_id=native-client-id"));

    let provider = KeyringGoogleAccessTokenProvider::new(
        store_with_envelope(Some(0), Some("test-refresh-token")),
        Some(oauth_config()),
    )
    .with_token_endpoint(format!("{}/token", server.base_url));
    provider.access_token().unwrap();
    let confidential_body = String::from_utf8_lossy(&server.recorded()[1].body).to_string();
    assert!(confidential_body.contains("client_secret=test-client-secret"));
}

/// #1087 route composition: the restart/refresh checkpoint through the
/// ordinary daemon operation shape. Daemon startup hydration and the
/// registration listing health-check each routed adapter via
/// `EventStoreAdapter::health_check()`; on the Google route that is the
/// EXACT production composition (`production_drive_event_store`
/// components). This regression drives that operation over a persisted
/// EXPIRED envelope with NO injected client config: the refresh identity
/// comes from the non-secret client id recorded in the envelope, the
/// rotated envelope is persisted BEFORE any Drive request departs, and
/// the real probe+list Drive I/O then succeeds.
#[test]
fn restart_health_operation_refreshes_envelope_then_performs_drive_io() {
    use bicameral_api::event_store::EventStoreAdapter as _;

    let request_order: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let order = request_order.clone();
    let server = StubServer::start(Box::new(move |request| {
        order.lock().unwrap().push(request.path_and_query.clone());
        if request.path_and_query.starts_with("/token") {
            let body = String::from_utf8_lossy(&request.body).to_string();
            // Envelope-carried native identity: client id present, no
            // client_secret ever.
            assert!(body.contains("client_id=envelope-native-client"));
            assert!(!body.contains("client_secret"));
            return (
                200,
                serde_json::json!({
                    "access_token": "post-restart-access-token",
                    "expires_in": 3599,
                    "token_type": "Bearer"
                })
                .to_string()
                .into_bytes(),
            );
        }
        if request
            .path_and_query
            .starts_with("/drive/v3/files/folder-registered")
        {
            return (
                200,
                serde_json::json!({
                    "id": "folder-registered",
                    "mimeType": "application/vnd.google-apps.folder",
                    "capabilities": {"canAddChildren": true}
                })
                .to_string()
                .into_bytes(),
            );
        }
        if request.path_and_query.starts_with("/drive/v3/files?") {
            return (
                200,
                serde_json::json!({"files": []}).to_string().into_bytes(),
            );
        }
        (404, b"{}".to_vec())
    }));

    // "Restart": the durable store already holds an EXPIRED envelope that
    // recorded its minting client identity; the daemon process has no
    // injected Google client environment.
    let store = Arc::new(InMemorySecretsStore::new());
    let envelope = bicameral_config::OAuthTokenEnvelopeV1::from_token_exchange(
        "pre-restart-access-token".into(),
        Some("durable-refresh-token".into()),
        Some(0),
        Some("Bearer".into()),
        None,
        chrono::Utc::now(),
    )
    .with_client_id(Some("envelope-native-client".into()));
    store
        .write_secret(
            bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
            bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
            &envelope.to_secret_string().unwrap(),
        )
        .unwrap();

    // EXACT production composition with injected endpoints only.
    let provider = Arc::new(
        KeyringGoogleAccessTokenProvider::new(store.clone(), None)
            .with_token_endpoint(format!("{}/token", server.base_url)),
    );
    let transport =
        Arc::new(HttpDriveApiTransport::new(provider).with_api_base(server.base_url.clone()));
    let adapter = bicameral_runtime::drive_api_event_store::DriveApiAdapter::new(
        "folder-registered",
        transport,
    )
    .unwrap();

    // The ordinary operation: startup hydration / registration listing
    // health check.
    let health = adapter.health_check().unwrap();
    assert!(health.reachable);

    // Refresh happened exactly once, BEFORE any Drive request.
    let requests = request_order.lock().unwrap().clone();
    let token_hits = requests
        .iter()
        .filter(|path| path.starts_with("/token"))
        .count();
    assert_eq!(token_hits, 1, "exactly one refresh generation");
    assert!(
        requests[0].starts_with("/token"),
        "refresh must precede Drive I/O: {requests:?}"
    );
    assert!(requests.len() >= 3, "probe and list must have executed");

    // The rotated envelope was persisted and keeps the minting identity.
    let stored = store
        .read_secret(
            bicameral_config::connections::connection_keys::GOOGLE_SOURCE_ID,
            bicameral_config::connections::connection_keys::OAUTH_TOKEN_KEY,
        )
        .unwrap();
    let parsed = bicameral_config::parse_stored_oauth_token(&stored).unwrap();
    assert_eq!(parsed.access_token(), Some("post-restart-access-token"));
    assert_eq!(parsed.refresh_token(), Some("durable-refresh-token"));
    let bicameral_config::StoredOAuthToken::Envelope(rotated) = parsed else {
        panic!("rotated credential must stay a versioned envelope");
    };
    assert_eq!(rotated.client_id.as_deref(), Some("envelope-native-client"));
}

/// #1087 route layer: the durable Google route serializes, reopens to the
/// Drive API composition, and fails closed on the atomic capability and
/// on unopenable records — with no I/O in any of these assertions.
#[test]
fn google_drive_api_route_resolves_production_composition_fail_closed() {
    use bicameral_runtime::project_registry::EventStoreRoute;

    let route = EventStoreRoute::GoogleDriveApi {
        folder_id: "folder-registered".into(),
    };
    let serialized = serde_json::to_string(&route).unwrap();
    assert!(serialized.contains("google_drive_api"));
    let parsed: EventStoreRoute = serde_json::from_str(&serialized).unwrap();
    assert_eq!(parsed, route);

    // Constructor-only identity assertions (no network, no keyring I/O).
    let dual = route.open_atomic();
    let error = dual
        .append_atomic_batch(
            bicameral_runtime::atomic_event_store::AtomicEventBatchV1::new(
                uuid::Uuid::from_u128(7),
                vec![event()],
            )
            .unwrap(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("atomic_batch_unsupported"));

    // A corrupt record (empty folder id) opens to a fail-closed adapter:
    // every operation is Unreachable and nothing silently substitutes a
    // local substrate.
    let corrupt = EventStoreRoute::GoogleDriveApi {
        folder_id: "   ".into(),
    };
    let dual = corrupt.open_atomic();
    let error = dual.health_check().unwrap_err();
    assert!(error.to_string().contains("unopenable"));
    let error = dual.replay().unwrap_err();
    assert!(error.to_string().contains("unopenable"));
}

/// #1101/#1134: concurrent callers over an expired credential produce
/// exactly one effective refresh generation; the waiter re-reads the
/// persisted envelope instead of issuing a second grant.
#[test]
fn concurrent_refresh_produces_single_generation() {
    let server = StubServer::start(Box::new(|_| {
        // Slow the winner slightly so the second caller reliably queues
        // on the refresh lock while the grant is in flight.
        std::thread::sleep(std::time::Duration::from_millis(150));
        (
            200,
            serde_json::json!({
                "access_token": "single-generation-token",
                "expires_in": 3599,
                "token_type": "Bearer"
            })
            .to_string()
            .into_bytes(),
        )
    }));

    let provider = Arc::new(
        KeyringGoogleAccessTokenProvider::new(
            store_with_envelope(Some(0), Some("test-refresh-token")),
            Some(oauth_config()),
        )
        .with_token_endpoint(format!("{}/token", server.base_url)),
    );

    let workers: Vec<_> = (0..2)
        .map(|_| {
            let provider = provider.clone();
            std::thread::spawn(move || provider.access_token().unwrap())
        })
        .collect();
    for worker in workers {
        assert_eq!(worker.join().unwrap(), "single-generation-token");
    }
    assert_eq!(
        server.recorded().len(),
        1,
        "exactly one refresh grant may reach the provider"
    );
}

// ── Transport tests against the stub ──────────────────────────────

/// Full adapter append against the stub: empty discovery, multipart
/// create with the canonical marker, exact readback confirmation.
#[test]
fn append_confirmed_roundtrip_with_marker_binding() {
    let uploaded: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let handler_uploaded = uploaded.clone();
    let server = StubServer::start(Box::new(move |request| {
        let path = request.path_and_query.as_str();
        if path.starts_with("/upload/drive/v3/files") {
            // Extract the media part (after the second part header) and
            // store it for exact readback.
            let body = request.body.clone();
            let marker = b"Content-Type: application/json\r\n\r\n";
            let positions: Vec<usize> = body
                .windows(marker.len())
                .enumerate()
                .filter(|(_, window)| *window == marker)
                .map(|(index, _)| index + marker.len())
                .collect();
            // The metadata part header carries "; charset=UTF-8", so the
            // bare application/json marker matches only the media part.
            let media_start = positions[0];
            let media_end = body[media_start..]
                .windows(4)
                .position(|window| window == b"\r\n--")
                .map(|position| media_start + position)
                .unwrap();
            *handler_uploaded.lock().unwrap() = Some(body[media_start..media_end].to_vec());
            (
                200,
                serde_json::json!({
                    "id": "file-1",
                    "name": format!("{EVENT_ID}.json"),
                    "headRevisionId": "rev-1"
                })
                .to_string()
                .into_bytes(),
            )
        } else if path.contains("alt=media") {
            (200, handler_uploaded.lock().unwrap().clone().unwrap())
        } else if path.starts_with("/drive/v3/files?") {
            (
                200,
                serde_json::json!({"files": []}).to_string().into_bytes(),
            )
        } else {
            (500, b"{}".to_vec())
        }
    }));

    let transport = Arc::new(
        HttpDriveApiTransport::new(Arc::new(StaticTokenProvider(
            "test-access-token-value".into(),
        )))
        .with_api_base(server.base_url.clone()),
    );
    let adapter = DriveApiAdapter::new("folder-1", transport).unwrap();
    let confirmation = adapter.append_confirmed(&event()).unwrap();
    assert!(confirmation.created);
    assert_eq!(confirmation.provider_file_id, "file-1");
    assert_eq!(confirmation.provider_version.as_deref(), Some("rev-1"));

    let recorded = server.recorded();
    // Discovery query is folder-bounded, shared-drive aware, and
    // marker-bound to the exact canonical filename.
    let discovery = &recorded[0];
    assert!(discovery.path_and_query.contains("supportsAllDrives=true"));
    let decoded = percent_decode(&discovery.path_and_query);
    assert!(decoded.contains("'folder-1' in parents"));
    assert!(decoded.contains(CANONICAL_EVENT_MARKER_KEY));
    assert!(decoded.contains(&format!("{EVENT_ID}.json")));

    // Upload stamped the canonical marker and parents in metadata, using
    // multipart/related as the Drive upload contract requires.
    let upload = recorded
        .iter()
        .find(|request| request.path_and_query.starts_with("/upload/"))
        .unwrap();
    let upload_body = String::from_utf8_lossy(&upload.body).to_string();
    assert!(upload_body.contains(CANONICAL_EVENT_MARKER_KEY));
    assert!(upload_body.contains("\"parents\":[\"folder-1\"]"));
    assert!(upload
        .content_type
        .as_deref()
        .unwrap()
        .starts_with("multipart/related"));
    // No token material in any recorded URL or body.
    for request in &recorded {
        assert!(!request.path_and_query.contains("test-access-token-value"));
        assert!(!String::from_utf8_lossy(&request.body).contains("Bearer test-access"));
    }
}

#[test]
fn provider_error_classes_map_to_typed_failures() {
    for (status, expected) in [
        (401u16, DriveTransportErrorKind::Authentication),
        (403, DriveTransportErrorKind::Authentication),
        (404, DriveTransportErrorKind::NotFound),
        (429, DriveTransportErrorKind::Unavailable),
        (500, DriveTransportErrorKind::Unavailable),
    ] {
        let server = StubServer::start(Box::new(move |_| (status, b"{}".to_vec())));
        let transport = HttpDriveApiTransport::new(Arc::new(StaticTokenProvider(
            "test-access-token-value".into(),
        )))
        .with_api_base(server.base_url.clone());
        let error = transport.download_file("file-1").unwrap_err();
        assert_eq!(error.kind, expected, "status {status}");
        assert!(!error.safe_message.contains("test-access-token-value"));
    }
}

#[test]
fn list_children_is_marker_bounded_and_paginates() {
    // Drive v3 rejects a key-only `appProperties has { key=... }` filter with
    // 400 Invalid Value (#1264), so the query is folder+trashed only and the
    // canonical-marker discrimination happens client-side. The stub returns a
    // marker-bearing event object plus an unrelated JSON file per page; only
    // the marker-bearing objects must survive.
    let server = StubServer::start(Box::new(|request| {
        if request.path_and_query.contains("pageToken") {
            (
                200,
                serde_json::json!({"files": [
                    {"id": "f2", "name": "b.json",
                     "appProperties": {CANONICAL_EVENT_MARKER_KEY: "b.json"}},
                    {"id": "unrelated-2", "name": "notes-2.json"}
                ]})
                .to_string()
                .into_bytes(),
            )
        } else {
            (
                200,
                serde_json::json!({
                    "files": [
                        {"id": "f1", "name": "a.json",
                         "appProperties": {CANONICAL_EVENT_MARKER_KEY: "a.json"}},
                        {"id": "unrelated-1", "name": "notes-1.json"}
                    ],
                    "nextPageToken": "page-2"
                })
                .to_string()
                .into_bytes(),
            )
        }
    }));
    let transport = HttpDriveApiTransport::new(Arc::new(StaticTokenProvider(
        "test-access-token-value".into(),
    )))
    .with_api_base(server.base_url.clone());

    let objects = transport.list_children("folder-1").unwrap();
    // Only the two marker-bearing objects survive the client-side filter.
    let ids: Vec<&str> = objects.iter().map(|o| o.file_id.as_str()).collect();
    assert_eq!(ids, vec!["f1", "f2"]);

    let recorded = server.recorded();
    assert_eq!(recorded.len(), 2);
    let decoded = percent_decode(&recorded[0].path_and_query);
    assert!(decoded.contains("'folder-1' in parents"));
    assert!(decoded.contains("trashed = false"));
    // The invalid key-only appProperties filter must never be sent.
    assert!(!decoded.contains("appProperties has"));
    // appProperties must be requested so the client-side filter has data.
    assert!(decoded.contains("appProperties"));
    assert!(recorded[1].path_and_query.contains("pageToken=page-2"));
}

#[test]
fn probe_folder_fails_closed() {
    let cases: Vec<(serde_json::Value, Option<DriveTransportErrorKind>)> = vec![
        (
            serde_json::json!({"id": "folder-1", "mimeType": "application/pdf",
                               "capabilities": {"canAddChildren": true}}),
            Some(DriveTransportErrorKind::InvalidResponse),
        ),
        (
            serde_json::json!({"id": "folder-1",
                               "mimeType": "application/vnd.google-apps.folder",
                               "trashed": true,
                               "capabilities": {"canAddChildren": true}}),
            Some(DriveTransportErrorKind::NotFound),
        ),
        (
            serde_json::json!({"id": "folder-1",
                               "mimeType": "application/vnd.google-apps.folder",
                               "capabilities": {"canAddChildren": false}}),
            Some(DriveTransportErrorKind::Authentication),
        ),
        (
            serde_json::json!({"id": "folder-1",
                               "mimeType": "application/vnd.google-apps.folder",
                               "capabilities": {"canAddChildren": true}}),
            None,
        ),
    ];
    for (payload, expected) in cases {
        let body = payload.to_string().into_bytes();
        let server = StubServer::start(Box::new(move |_| (200, body.clone())));
        let transport = HttpDriveApiTransport::new(Arc::new(StaticTokenProvider(
            "test-access-token-value".into(),
        )))
        .with_api_base(server.base_url.clone());
        let result = transport.probe_folder("folder-1");
        match expected {
            None => assert!(result.is_ok()),
            Some(kind) => assert_eq!(result.unwrap_err().kind, kind),
        }
    }
}

#[test]
fn production_composition_requires_folder_authority() {
    // Empty folder id (no validated resource) fails closed at
    // construction; the composition function never invents one.
    let result = production_drive_event_store("  ", Arc::new(InMemorySecretsStore::new()), None);
    assert!(result.is_err());
}
