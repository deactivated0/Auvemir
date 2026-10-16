use auvemir_core::{
    database::Store,
    model::{Media, MediaKind},
    scrobble::{Listen, Service},
    servers::Server,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
async fn mock(body: &str) -> (String, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = body.to_owned();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let n = socket.read(&mut buffer).await.unwrap();
            request.extend_from_slice(&buffer[..n]);
            if n == 0 {
                break;
            }
            if let Some(end) = request.windows(4).position(|x| x == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&request[..end]);
                let length = header
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, n)| n.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        String::from_utf8(request).unwrap()
    });
    (format!("http://{addr}/"), task)
}
async fn mock_many(bodies: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let mut requests = Vec::with_capacity(bodies.len());
        for body in bodies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let n = socket.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..n]);
                if n == 0 || request.windows(4).any(|x| x == b"\r\n\r\n") {
                    break;
                }
            }
            requests.push(String::from_utf8(request).unwrap());
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        }
        requests
    });
    (format!("http://{addr}/"), task)
}
fn listen(id: &str) -> Listen {
    Listen {
        media_id: id.into(),
        title: "Track".into(),
        artist: "Artist".into(),
        album: "Album".into(),
        duration_ms: 300_000,
        started_at: 1700000000,
        listened_ms: 150_000,
    }
}
#[test]
fn durable_outbox_deduplicates_and_backs_off() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let mut store = Store::open(&path).unwrap();
    let media = Media::new(MediaKind::LocalMusic, "local", "/track.flac", "Track");
    store.upsert(std::slice::from_ref(&media)).unwrap();
    let mut l = listen(&media.id);
    let id = store
        .record_listen(&l, &["service-a".into(), "service-b".into()])
        .unwrap();
    assert_eq!(store.record_listen(&l, &["service-a".into()]).unwrap(), id);
    assert_eq!(store.pending_scrobbles().unwrap(), 2);
    l.listened_ms = 240_000;
    store.record_listen(&l, &[]).unwrap();
    drop(store);
    let store = Store::open(path).unwrap();
    assert_eq!(store.listening_history().unwrap().len(), 1);
    assert_eq!(store.listening_history().unwrap()[0].listened_ms, 240_000);
    let pending = store.scrobbles(100).unwrap();
    assert_eq!(pending.len(), 2);
    store.scrobble_result(pending[0].0, false, 100).unwrap();
    assert_eq!(store.scrobbles(129).unwrap().len(), 1);
    assert_eq!(store.scrobbles(130).unwrap().len(), 2);
    store.scrobble_result(pending[0].0, true, 130).unwrap();
    assert_eq!(store.pending_scrobbles().unwrap(), 1);
}
#[test]
fn history_import_keeps_metadata_without_original_file() {
    let store = Store::memory().unwrap();
    let l = listen("unavailable-file");
    store.record_listen(&l, &[]).unwrap();
    store.record_listen(&l, &[]).unwrap();
    let history = store.listening_history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].artist, "Artist");
}
#[test]
fn scrobble_threshold_counts_listening_time() {
    let mut l = listen("a");
    assert!(l.eligible());
    l.listened_ms = 149_999;
    assert!(!l.eligible());
    l.duration_ms = 20_000;
    l.listened_ms = 20_000;
    assert!(!l.eligible());
    l.duration_ms = 600_000;
    l.listened_ms = 240_000;
    assert!(l.eligible());
    l.artist.clear();
    assert!(!l.eligible());
}
#[tokio::test]
async fn subsonic_auth_is_excluded_from_media_identity() {
    let (endpoint,task)=mock(r#"{"subsonic-response":{"status":"ok","searchResult3":{"song":[{"id":"42","title":"Track","artist":"Artist","album":"Album","duration":60}]}}}"#).await;
    let server = Server {
        id: "profile".into(),
        name: "Test".into(),
        protocol: "subsonic".into(),
        endpoint,
        username: "user".into(),
    };
    let media = server.search("secret-password", "artist").await.unwrap();
    assert_eq!(media.len(), 1);
    let json = serde_json::to_string(&media).unwrap();
    assert!(!json.contains("secret-password"));
    assert!(!json.contains("t="));
    assert_eq!(media[0].duration_ms, 60_000);
    assert!(!media[0].seekable());
    let request = task.await.unwrap();
    assert!(request.contains("search3.view?"));
    assert!(request.contains("u=user"));
    assert!(request.contains("query=artist"));
    assert!(!request.contains("secret-password"));
    let stream = server.resolve("secret-password", &media[0]).await.unwrap();
    assert!(stream.location.contains("id=42"));
}
#[tokio::test]
async fn subsonic_search_page_uses_server_offset_and_reports_continuation() {
    let songs = (0..101)
        .map(|n| {
            serde_json::json!({
                "id": format!("song-{n}"),
                "title": format!("Track {n}"),
                "duration": 60
            })
        })
        .collect::<Vec<_>>();
    let body = serde_json::json!({
        "subsonic-response": {
            "status": "ok",
            "searchResult3": { "song": songs }
        }
    })
    .to_string();
    let (endpoint, task) = mock(&body).await;
    let server = Server {
        id: "profile".into(),
        name: "Test".into(),
        protocol: "subsonic".into(),
        endpoint,
        username: "user".into(),
    };
    let page = server.search_page("secret", "track", 100).await.unwrap();
    assert_eq!(page.items.len(), 100);
    assert!(page.has_more);
    let request = task.await.unwrap();
    assert!(request.contains("songCount=101"));
    assert!(request.contains("songOffset=100"));
}
#[tokio::test]
async fn invidious_search_uses_page_parameter_and_exposes_continuation() {
    let bodies = (100..200)
        .step_by(20)
        .map(|start| {
            let videos = (start..start + 20)
                .map(|n| {
                    serde_json::json!({
                        "videoId": format!("id{n:09}"),
                        "title": format!("Video {n}"),
                        "author": "Artist",
                        "lengthSeconds": 60
                    })
                })
                .collect::<Vec<_>>();
            serde_json::to_string(&videos).unwrap()
        })
        .collect();
    let (endpoint, task) = mock_many(bodies).await;
    let server = Server {
        id: "optional".into(),
        name: "Optional".into(),
        protocol: "invidious".into(),
        endpoint,
        username: String::new(),
    };
    let page = server.search_page("", "music", 100).await.unwrap();
    assert_eq!(page.items.len(), 100);
    assert_eq!(page.items.first().unwrap().title, "Video 100");
    assert_eq!(page.items.last().unwrap().title, "Video 199");
    assert!(page.has_more);
    let requests = task.await.unwrap();
    assert_eq!(requests.len(), 5);
    for (index, request) in requests.iter().enumerate() {
        assert!(request.contains(&format!("page={}", index + 6)));
        assert!(request.contains("type=video"));
    }
}
#[tokio::test]
async fn listenbrainz_compatible_request_uses_token_header() {
    let (endpoint, task) = mock(r#"{"status":"ok"}"#).await;
    let service = Service {
        id: "one".into(),
        name: "Test".into(),
        protocol: "listenbrainz".into(),
        endpoint,
        enabled: true,
    };
    auvemir_core::scrobble::submit(&service, "test-token", &listen("file"))
        .await
        .unwrap();
    let request = task.await.unwrap();
    assert!(request.starts_with("POST /1/submit-listens"));
    assert!(
        request
            .to_lowercase()
            .contains("authorization: token test-token")
    );
}
#[tokio::test]
async fn webdav_directory_hides_folders_and_encodes_file_identity() {
    let (endpoint,task)=mock(r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:"><d:response><d:href>/album/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response><d:response><d:href>/album/tone.flac</d:href><d:propstat><d:prop><d:displayname>Tone</d:displayname><d:resourcetype/></d:prop></d:propstat></d:response></d:multistatus>"#).await;
    let server = Server {
        id: "dav".into(),
        name: "WebDAV".into(),
        protocol: "webdav".into(),
        endpoint,
        username: "user".into(),
    };
    let tracks = server.search("password", "").await.unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].title, "Tone");
    let stream = server.resolve("password", &tracks[0]).await.unwrap();
    assert!(stream.location.ends_with("/album/tone.flac"));
    assert!(task.await.unwrap().starts_with("PROPFIND"));
}
#[tokio::test]
async fn webdav_search_page_reports_and_returns_next_page() {
    let mut rows = String::from(r#"<d:multistatus xmlns:d="DAV:">"#);
    for n in 0..101 {
        rows.push_str(&format!(
            r#"<d:response><d:href>/track-{n}.mp3</d:href><d:propstat><d:prop><d:displayname>Track {n}</d:displayname><d:resourcetype/></d:prop></d:propstat></d:response>"#
        ));
    }
    rows.push_str("</d:multistatus>");
    let (endpoint, task) = mock(&rows).await;
    let server = Server {
        id: "dav".into(),
        name: "WebDAV".into(),
        protocol: "webdav".into(),
        endpoint,
        username: "user".into(),
    };
    let page = server.search_page("password", "", 0).await.unwrap();
    assert_eq!(page.items.len(), 100);
    assert!(page.has_more);
    assert_eq!(page.items[0].title, "Track 0");
    task.await.unwrap();
}
#[tokio::test]
async fn webdav_folders_can_be_opened_without_leaving_the_configured_origin() {
    let root = r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/</d:href><d:propstat><d:prop><d:displayname>Library</d:displayname><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response><d:response><d:href>/Music/</d:href><d:propstat><d:prop><d:displayname>Music</d:displayname><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response></d:multistatus>"#.to_owned();
    let nested = r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/Music/Tone.mp3</d:href><d:propstat><d:prop><d:displayname>Tone</d:displayname><d:resourcetype/></d:prop></d:propstat></d:response></d:multistatus>"#.to_owned();
    let (endpoint, task) = mock_many(vec![root, nested]).await;
    let server = Server {
        id: "dav".into(),
        name: "WebDAV".into(),
        protocol: "webdav".into(),
        endpoint,
        username: "user".into(),
    };
    let root_page = server.search_page("password", "", 0).await.unwrap();
    assert!(root_page.items.is_empty());
    assert_eq!(root_page.folders.len(), 1);
    assert_eq!(root_page.folders[0].name, "Music");
    assert_eq!(root_page.folders[0].path, "/Music/");
    let nested_page = server
        .search_directory_page("password", "", 0, Some(&root_page.folders[0].path))
        .await
        .unwrap();
    assert_eq!(nested_page.items.len(), 1);
    assert_eq!(nested_page.items[0].title, "Tone");
    let requests = task.await.unwrap();
    assert!(requests[0].starts_with("PROPFIND / HTTP/1.1"));
    assert!(requests[1].starts_with("PROPFIND /Music/ HTTP/1.1"));
}

#[tokio::test]
async fn audiobookshelf_book_expands_to_all_tracks() {
    let (endpoint,task)=mock(r#"{"media":{"tracks":[{"title":"One.mp3","duration":120,"contentUrl":"/s/item/book/one.mp3"},{"title":"Two.mp3","duration":180,"contentUrl":"/s/item/book/two.mp3"}]},"userMediaProgress":{"currentTime":42.25,"isFinished":false}}"#).await;
    let server = Server {
        id: "abs".into(),
        name: "Books".into(),
        protocol: "audiobookshelf".into(),
        endpoint,
        username: String::new(),
    };
    let mut book = Media::new(
        MediaKind::Audiobook,
        "server:abs",
        "auvemir://server/abs/book",
        "Book",
    );
    book.album = "Book".into();
    book.artist = "Author".into();
    book.group_id = "book".into();
    let (tracks, progress) = server
        .expand_book_with_progress("private-token", &book)
        .await
        .unwrap();
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[1].track_number, 2);
    assert_eq!(tracks[1].duration_ms, 180000);
    assert_ne!(tracks[0].id, tracks[1].id);
    assert_eq!(progress.unwrap().current_time_ms, 42_250);
    assert!(
        !serde_json::to_string(&tracks)
            .unwrap()
            .contains("private-token")
    );
    let request = task.await.unwrap();
    assert!(request.starts_with("GET /api/items/book?expanded=1&include=progress"));
    assert!(
        request
            .to_lowercase()
            .contains("authorization: bearer private-token")
    );
}
#[tokio::test]
async fn audiobookshelf_progress_update_is_authenticated_and_uses_seconds() {
    let (endpoint, task) = mock(r#"{"success":true}"#).await;
    let server = Server {
        id: "abs".into(),
        name: "Books".into(),
        protocol: "audiobookshelf".into(),
        endpoint,
        username: String::new(),
    };
    server
        .update_book_progress("private-token", "book-id", 1_500, 10_000, false)
        .await
        .unwrap();
    let request = task.await.unwrap();
    assert!(request.starts_with("PATCH /api/me/progress/book-id HTTP/1.1"));
    assert!(
        request
            .to_lowercase()
            .contains("authorization: bearer private-token")
    );
    assert!(request.contains(r#""currentTime":1.5"#));
    assert!(request.contains(r#""duration":10.0"#));
    assert!(request.contains(r#""progress":0.15"#));
}
#[tokio::test]
async fn audiobookshelf_search_walks_catalog_pages_to_requested_offset() {
    let item = |n| {
        serde_json::json!({
            "id": format!("book-{n}"),
            "media": {
                "metadata": { "title": format!("Book {n}"), "authorName": "Author" },
                "duration": 60.0
            }
        })
    };
    let first_page = (0..100).map(item).collect::<Vec<_>>();
    let second_page = (100..102).map(item).collect::<Vec<_>>();
    let (endpoint, task) = mock_many(vec![
        serde_json::json!({ "libraries": [{ "id": "library" }] }).to_string(),
        serde_json::json!({ "results": first_page, "total": 102 }).to_string(),
        serde_json::json!({ "results": second_page, "total": 102 }).to_string(),
    ])
    .await;
    let server = Server {
        id: "abs".into(),
        name: "Books".into(),
        protocol: "audiobookshelf".into(),
        endpoint,
        username: String::new(),
    };
    let page = server.search_page("token", "", 100).await.unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].title, "Book 100");
    assert_eq!(page.items[1].title, "Book 101");
    assert!(!page.has_more);
    let requests = task.await.unwrap();
    assert!(requests[1].contains("page=0"));
    assert!(requests[2].contains("page=1"));
}
#[tokio::test]
async fn unofficial_catalog_rejects_restricted_media() {
    let (endpoint,task)=mock(r#"{"paid":true,"adaptiveFormats":[{"type":"audio/mp4","url":"http://127.0.0.1/private"}]}"#).await;
    let server = Server {
        id: "optional".into(),
        name: "Optional".into(),
        protocol: "invidious".into(),
        endpoint,
        username: String::new(),
    };
    let media = Media::new(
        MediaKind::OnlineMusic,
        "server:optional",
        "auvemir://server/optional/abcdefghijk",
        "Restricted",
    );
    assert!(
        server
            .resolve("", &media)
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("restricted")
    );
    task.await.unwrap();
}
#[tokio::test]
async fn service_error_with_http_success_remains_retryable() {
    let (endpoint, task) = mock(r#"{"status":"error","error":"invalid token"}"#).await;
    let service = Service {
        id: "one".into(),
        name: "Test".into(),
        protocol: "listenbrainz".into(),
        endpoint,
        enabled: true,
    };
    assert!(
        auvemir_core::scrobble::submit(&service, "private-token", &listen("file"))
            .await
            .is_err()
    );
    task.await.unwrap();
}

#[tokio::test]
async fn now_playing_omits_listen_timestamp() {
    let (endpoint, task) = mock(r#"{"status":"ok"}"#).await;
    let service = Service {
        id: "test".into(),
        name: "Test".into(),
        protocol: "listenbrainz".into(),
        endpoint,
        enabled: true,
    };
    auvemir_core::scrobble::now_playing(&service, "test-token", &listen("track"))
        .await
        .unwrap();
    let request = task.await.unwrap();
    assert!(request.contains("playing_now"));
    assert!(!request.contains("listened_at"));
}
#[tokio::test]
async fn libre_fm_now_playing_uses_signed_modern_api() {
    let (endpoint, task) = mock(r#"{"nowplaying":{"ignoredMessage":{"code":"0"}}}"#).await;
    let service = Service {
        id: "test".into(),
        name: "Test".into(),
        protocol: "librefm".into(),
        endpoint,
        enabled: true,
    };
    auvemir_core::scrobble::now_playing(
        &service,
        r#"{"api_key":"key","api_secret":"secret","session_key":"session"}"#,
        &listen("track"),
    )
    .await
    .unwrap();
    let request = task.await.unwrap();
    assert!(request.contains("track.updateNowPlaying"));
    assert!(request.contains("api_sig="));
    assert!(!request.contains("timestamp="));
    assert!(!request.contains("api_secret="));
}
