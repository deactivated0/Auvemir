use crate::{
    audio::{self, Audio, Prepared},
    recorder::Recorder,
};
use anyhow::{Context, Result, bail};
use auvemir_core::{
    database::Store,
    downloads::Downloader,
    model::{CatalogBook, Media, MediaKind, MediaSort, Station, format_time, stable_id},
    providers::Providers,
    queue::Queue,
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};
slint::include_modules!();

enum Command {
    Navigate(i32),
    Sort(i32),
    Theme(i32),
    Preference(String, bool),
    SelectServer(i32),
    ViewResult(u64, Box<Command>),
    ScanFinished(u64, std::result::Result<(), String>),
    Play(String),
    Action(String),
    Import(String, i32),
    OpenFile(String),
    Search(String, i32),
    Subscribe(String, i32),
    SaveStation(Station),
    DeleteStation(String),
    EditStation(String),
    Favorite(String),
    Seek(f32),
    Volume(f32),
    Speed(f32),
    Eq(i32, f32),
    LayerVolume(String, f32),
    AmbientPreset(String, String),
    SavePlaylist(String),
    PlaylistAction(String, String, String),
    OpenPlaylist(String),
    Collection(String),
    Bookmark(String),
    Sleep(String),
    Files(String, String),
    Configure(String, String, String, String, String),
    RemotePage(
        u64,
        usize,
        std::result::Result<auvemir_core::servers::ServerPage, String>,
    ),
    BookTracks(Vec<Media>, Option<auvemir_core::servers::RemoteProgress>),
    RemoteProgressSynced(String, std::result::Result<(), String>),
    EmbeddedChapters(
        String,
        u64,
        std::result::Result<Vec<auvemir_core::library::EmbeddedChapter>, String>,
    ),
    MediaControl(crate::controls::MediaControlEvent),
    Scrobbled(Vec<(i64, bool)>),
    ArtworkReady(String),
    Scanned(u64, auvemir_core::library::Scanned),
    Catalog(Vec<CatalogBook>),
    Stations(Vec<Station>),
    Collections(Vec<(String, String, String)>),
    Feed(String, String, MediaKind, Vec<Media>),
    Prepared(u64, Media, u64, Prepared),
    PrepareFailed(u64, String),
    RoomAction(String, String),
    RoomReady(u64, std::result::Result<crate::rooms::Connection, String>),
    RoomWake(u64),
    Layer(u64, String, Prepared),
    CaptureFinished(u64),
    Lyrics(String, String),
    LyricsFetched(String, u64, std::result::Result<String, String>),
    SaveLyrics(String, String),
    SeekLyric(String, i32),
    Downloaded(String, PathBuf),
    DownloadFailed(String),
    Notice(String),
    ConfigureSync(String, String, String, String),
    SyncFinished(std::result::Result<auvemir_core::sync::Snapshot, String>),
    CastDevices(Vec<auvemir_core::cast::Device>),
    ConnectCast(String),
    ConfigureDiscord(String),
    TagDraft(Media, auvemir_core::library::Tags),
    SaveTags(String, auvemir_core::library::Tags),
    Tagged(Media),
    ArtSelected(Media),
    Metadata(Media, Vec<auvemir_core::metadata::Candidate>),
    ConfigureApi(String, String),
    ImportReference(String, String),
    ReferenceLoaded(auvemir_core::metadata::ReferencePlaylist),
    CastEvent(u64, crate::casting::Event),
    CastSource(u64, u64, Media, u64, auvemir_core::servers::Stream),
    Native(String, String, i64),
    #[cfg(target_os = "android")]
    UiAttached,
    Shutdown,
}

fn theme_id(index: i32) -> &'static str {
    match index {
        1 => "ocean",
        2 => "forest",
        3 => "violet",
        4 => "white",
        5 => "cloud",
        6 => "rose",
        7 => "mint",
        _ => "amoled",
    }
}

fn theme_index(id: &str) -> Option<i32> {
    match id {
        "amoled" => Some(0),
        "ocean" => Some(1),
        "forest" => Some(2),
        "violet" => Some(3),
        "white" => Some(4),
        "cloud" => Some(5),
        "rose" => Some(6),
        "mint" => Some(7),
        _ => None,
    }
}

#[cfg(target_os = "android")]
#[derive(Clone)]
struct MobileEngine {
    sender: SyncSender<Command>,
    ui: crate::ui_target::Target,
    alive: Arc<AtomicBool>,
}
#[cfg(target_os = "android")]
fn background_engine() -> Result<MobileEngine> {
    static ENGINE: std::sync::OnceLock<std::sync::Mutex<Option<MobileEngine>>> =
        std::sync::OnceLock::new();
    let mut slot = ENGINE
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| anyhow::anyhow!("Audio worker lock failed"))?;
    if let Some(engine) = &*slot
        && engine.alive.load(Ordering::Acquire)
    {
        return Ok(engine.clone());
    }
    let base = crate::platform::data_dir()?;
    std::fs::create_dir_all(&base)?;
    let (sender, receiver) = mpsc::sync_channel(64);
    let ui = crate::ui_target::Target::default();
    let alive = Arc::new(AtomicBool::new(true));
    let worker_tx = sender.clone();
    let worker_ui = ui.clone();
    let worker_alive = alive.clone();
    std::thread::Builder::new()
        .name("auvemir-state".into())
        .spawn(move || {
            if let Err(error) =
                Worker::new(base, worker_tx, worker_ui.clone()).and_then(|w| w.run(receiver))
            {
                tracing::error!("Audio worker stopped: {error:#}");
                #[cfg(target_os = "android")]
                log::error!("Audio worker stopped: {error:#}");
                let message = format!("Application error: {error}");
                let _ = worker_ui.upgrade_in_event_loop(move |u| u.set_status(message.into()));
            }
            // Serialize teardown with a later Activity/service starting a replacement worker.
            let _slot = ENGINE.get().and_then(|slot| slot.lock().ok());
            crate::platform::unregister();
            crate::controls::shutdown();
            crate::platform::shutdown();
            worker_alive.store(false, Ordering::Release);
        })?;
    let native_tx = sender.clone();
    crate::platform::register(move |a, b, n| {
        let _ = native_tx.try_send(Command::Native(a, b, n));
    });
    let engine = MobileEngine { sender, ui, alive };
    *slot = Some(engine.clone());
    Ok(engine)
}
#[cfg(target_os = "android")]
pub(crate) fn start_background() -> Result<()> {
    background_engine().map(|_| ())
}

fn submit(sender: &SyncSender<Command>, cmd: Command, weak: &slint::Weak<MainWindow>) {
    match &cmd {
        Command::Favorite(id) => tracing::debug!(media_id = %id, "Toggle favorite input"),
        Command::Play(id) => tracing::debug!(media_id = %id, "Play input"),
        _ => {}
    }
    if sender.try_send(cmd).is_err() {
        let _ = weak.upgrade_in_event_loop(|ui| {
            ui.set_status("The task queue is busy. Try again shortly.".into())
        });
    }
}

fn audiobook_resume_offset(
    tracks: &[Media],
    progress: Option<auvemir_core::servers::RemoteProgress>,
) -> (usize, u64) {
    let Some(progress) = progress.filter(|progress| !progress.is_finished) else {
        return (0, 0);
    };
    let total = tracks
        .iter()
        .fold(0u64, |sum, track| sum.saturating_add(track.duration_ms));
    if total == 0 || progress.current_time_ms >= total {
        return (0, 0);
    }
    let mut remaining = progress.current_time_ms;
    for (index, track) in tracks.iter().enumerate() {
        if remaining < track.duration_ms {
            return (index, remaining);
        }
        remaining = remaining.saturating_sub(track.duration_ms);
    }
    (0, 0)
}

fn webdav_directory_id(server_id: &str, path: &str) -> String {
    let encoded = url::form_urlencoded::byte_serialize(path.as_bytes()).collect::<String>();
    format!("webdav-dir:{server_id}:{encoded}")
}

fn parse_webdav_directory_id(id: &str) -> Result<(String, String)> {
    let directory = id
        .strip_prefix("webdav-dir:")
        .context("Invalid WebDAV folder identity")?;
    let (server_id, encoded_path) = directory
        .split_once(':')
        .context("Invalid WebDAV folder identity")?;
    let path = url::form_urlencoded::parse(format!("path={encoded_path}").as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
        .context("Invalid WebDAV folder path")?;
    Ok((server_id.to_owned(), path))
}

fn webdav_parent_path(path: &str) -> Option<String> {
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return None;
    }
    let parent = path.rsplit_once('/')?.0;
    Some(if parent.is_empty() {
        "/".into()
    } else {
        format!("{parent}/")
    })
}

pub fn run() -> Result<()> {
    let args: Vec<String> =
        if cfg!(target_os = "android") || cfg!(all(target_os = "ios", not(target_abi = "sim"))) {
            Vec::new()
        } else {
            std::env::args().skip(1).collect()
        };
    if args.first().is_some_and(|s| s == "--help" || s == "-h") {
        println!(
            "Auvemir {}\nNative audio player\n\nUsage: auvemir [FILE_OR_FOLDER]\n  --decode FILE       Check decoding\n  --play FILE         Play for up to 60 seconds\n  --fingerprint FILE  Print an AcoustID fingerprint\n  --version           Print version\n  --help              Show this help",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "--version" || s == "-V") {
        println!("Auvemir {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "--play") {
        return play_cli(Path::new(
            args.get(1).context("Usage: auvemir --play FILE")?,
        ));
    }
    if args.first().is_some_and(|s| s == "--fingerprint") {
        let (duration, fingerprint) = crate::fingerprint::calculate(Path::new(
            args.get(1).context("Usage: auvemir --fingerprint FILE")?,
        ))?;
        println!("{duration}\t{fingerprint}");
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "--decode") {
        return decode_check(Path::new(
            args.get(1).context("Usage: auvemir --decode FILE")?,
        ));
    }
    #[cfg(not(target_os = "android"))]
    let base = crate::platform::data_dir()?;
    #[cfg(not(target_os = "android"))]
    std::fs::create_dir_all(&base)?;
    let ui = MainWindow::new()?;
    ui.set_reduced_motion(crate::motion::system_reduced());
    ui.set_native_mobile(cfg!(any(target_os = "android", target_os = "ios")));
    ui.set_native_airplay(cfg!(target_os = "ios"));
    #[cfg(not(target_os = "android"))]
    let (sender, receiver) = mpsc::sync_channel(64);
    #[cfg(target_os = "android")]
    let engine = background_engine()?;
    #[cfg(target_os = "android")]
    let sender = engine.sender.clone();
    let weak = ui.as_weak();
    macro_rules! bind {($method:ident,$($arg:ident),* =>$command:expr)=>{{let tx=sender.clone();let w=weak.clone();ui.$method(move|$($arg),*|submit(&tx,$command,&w));}};}
    bind!(on_navigate,page=>Command::Navigate(page));
    bind!(on_sort_library,index=>Command::Sort(index));
    bind!(on_select_theme,index=>Command::Theme(index));
    bind!(on_preference,key,value=>Command::Preference(key.to_string(),value));
    bind!(on_select_server,index=>Command::SelectServer(index));
    ui.on_quit(|| {
        let _ = slint::quit_event_loop();
    });
    bind!(on_play_track,id=>Command::Play(id.to_string()));
    bind!(on_action,a=>Command::Action(a.to_string()));
    bind!(on_import_audio,path,page=>Command::Import(path.to_string(),page));
    bind!(on_remote_search,q,page=>Command::Search(q.to_string(),page));
    bind!(on_subscribe,url,page=>Command::Subscribe(url.to_string(),page));
    bind!(on_delete_station,id=>Command::DeleteStation(id.to_string()));
    bind!(on_edit_station,id=>Command::EditStation(id.to_string()));
    bind!(on_favorite_track,id=>Command::Favorite(id.to_string()));
    bind!(on_seek,f=>Command::Seek(f));
    bind!(on_set_volume,v=>Command::Volume(v));
    bind!(on_set_speed,v=>Command::Speed(v));
    bind!(on_set_eq,index,gain=>Command::Eq(index,gain));
    bind!(on_layer_volume,id,v=>Command::LayerVolume(id.to_string(),v));
    bind!(on_ambient_preset,a,name=>Command::AmbientPreset(a.to_string(),name.to_string()));
    bind!(on_save_lyrics,id,text=>Command::SaveLyrics(id.to_string(),text.to_string()));
    bind!(on_seek_lyric,id,index=>Command::SeekLyric(id.to_string(),index));
    bind!(on_save_playlist,name=>Command::SavePlaylist(name.to_string()));
    bind!(on_playlist_action,a,id,name=>Command::PlaylistAction(a.to_string(),id.to_string(),name.to_string()));
    bind!(on_open_collection,id=>Command::Collection(id.to_string()));
    bind!(on_open_playlist,id=>Command::OpenPlaylist(id.to_string()));
    bind!(on_bookmark,name=>Command::Bookmark(name.to_string()));
    bind!(on_set_sleep,m=>Command::Sleep(m.to_string()));
    bind!(on_files_action,a,path=>Command::Files(a.to_string(),path.to_string()));
    bind!(on_configure_service,kind,name,url,user,secret=>Command::Configure(kind.to_string(),name.to_string(),url.to_string(),user.to_string(),secret.to_string()));
    bind!(on_save_station,id,name,url,country,language,tags,logo=>Command::SaveStation(Station{id:if id.is_empty(){format!("custom:{}",stable_id("station",&format!("{}:{}",url,std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos())))}else{id.to_string()},name:name.to_string(),url:url.to_string(),country:country.to_string(),language:language.to_string(),tags:tags.to_string(),logo:(!logo.is_empty()).then(||logo.to_string()),custom:true,favorite:false}));
    #[cfg(not(target_os = "android"))]
    let native_tx = sender.clone();
    #[cfg(not(target_os = "android"))]
    crate::platform::register(move |a, b, n| {
        let _ = native_tx.try_send(Command::Native(a, b, n));
    });
    bind!(on_configure_api,kind,secret=>Command::ConfigureApi(kind.to_string(),secret.to_string()));
    bind!(on_room_action,action,value=>Command::RoomAction(action.to_string(),value.to_string()));
    bind!(on_import_reference,provider,url=>Command::ImportReference(provider.to_string(),url.to_string()));
    bind!(on_configure_discord,id=>Command::ConfigureDiscord(id.to_string()));
    bind!(on_save_tags,id,title,artist,album,genre,composer=>Command::SaveTags(id.to_string(),auvemir_core::library::Tags{title:title.to_string(),artist:artist.to_string(),album:album.to_string(),genre:genre.to_string(),composer:composer.to_string()}));
    bind!(on_connect_cast,address=>Command::ConnectCast(address.to_string()));
    bind!(on_configure_sync,url,user,password,pass=>Command::ConfigureSync(url.to_string(),user.to_string(),password.to_string(),pass.to_string()));
    #[cfg(not(target_os = "android"))]
    let worker_tx = sender.clone();
    #[cfg(not(target_os = "android"))]
    let worker_weak = crate::ui_target::Target::new(weak.clone());
    #[cfg(target_os = "android")]
    let _attachment = engine.ui.attach(weak.clone());
    #[cfg(target_os = "android")]
    sender
        .send(Command::UiAttached)
        .map_err(|_| anyhow::anyhow!("Audio worker unavailable"))?;
    ui.show()?;
    let control_tx = sender.clone();
    crate::controls::init(ui.window(), move |event| {
        let _ = control_tx.try_send(Command::MediaControl(event));
    });
    #[cfg(not(target_os = "android"))]
    let worker = std::thread::Builder::new()
        .name("auvemir-state".into())
        .spawn(move || {
            if let Err(e) =
                Worker::new(base, worker_tx, worker_weak.clone()).and_then(|w| w.run(receiver))
            {
                let message = format!("Application error: {e}");
                let _ = worker_weak.upgrade_in_event_loop(move |u| u.set_status(message.into()));
            }
        })?;
    if let Some(path) = args.first().filter(|s| !s.starts_with('-')) {
        sender
            .send(if Path::new(path).is_file() {
                Command::OpenFile(path.clone())
            } else {
                Command::Import(path.clone(), 1)
            })
            .map_err(|_| anyhow::anyhow!("State worker unavailable"))?;
    }
    if std::env::var("AUVEMIR_SCREENSHOT").is_ok() {
        if let Ok(page) = std::env::var("AUVEMIR_SCREENSHOT_PAGE")
            .unwrap_or_default()
            .parse::<i32>()
        {
            let _ = sender.send(Command::Navigate(page));
        }
        if let Ok(section) = std::env::var("AUVEMIR_SCREENSHOT_SETTINGS")
            .unwrap_or_default()
            .parse::<i32>()
        {
            ui.set_settings_section(section);
        }
        if std::env::var("AUVEMIR_SCREENSHOT_PLAYER").is_ok() {
            ui.set_player_expanded(true);
        }
    }
    let capture = crate::capture::Capture::start(&ui);
    ui.run()?;
    #[cfg(not(target_os = "android"))]
    {
        crate::platform::unregister();
        crate::controls::shutdown();
        crate::platform::shutdown();
        // Send after the event loop stops; commands in flight cannot dereference a destroyed UI.
        let _ = sender.send(Command::Shutdown);
        drop(sender);
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("State worker failed"))?;
    }
    capture.finish()?;
    Ok(())
}

fn decode_check(path: &Path) -> Result<()> {
    use rodio::Source;
    let mut d = audio::decode_file(path)?;
    let channels = d.channels();
    let rate = d.sample_rate();
    let samples = d.by_ref().count();
    if samples == 0 {
        bail!("Decoder produced no samples");
    }
    println!("Decoded {samples} samples · {channels} channels · {rate} Hz");
    Ok(())
}

fn play_cli(path: &Path) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let m = auvemir_core::library::read_media(path, MediaKind::LocalMusic)?;
    let p = runtime.block_on(audio::prepare(m, 0, false))?;
    let mut engine = Audio::new();
    engine.play(p, 0)?;
    let started = Instant::now();
    while engine.playing() && started.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(20));
    }
    println!(
        "Native output playback ended after {:.3}s; position {}",
        started.elapsed().as_secs_f64(),
        format_time(engine.position())
    );
    engine.stop();
    runtime.shutdown_background();
    Ok(())
}

struct Worker {
    base: PathBuf,
    db: Store,
    tx: SyncSender<Command>,
    ui: crate::ui_target::Target,
    runtime: tokio::runtime::Runtime,
    audio: Audio,
    queue: Queue,
    current: Option<Media>,
    page: i32,
    sort: MediaSort,
    theme_index: i32,
    reduce_motion: bool,
    loading: bool,
    home_recent: bool,
    view_generation: u64,
    scan_generation: u64,
    scanning: bool,
    scan_imported: usize,
    scan_errors: usize,
    offset: usize,
    query: String,
    remote_has_more: bool,
    remote_request_generation: u64,
    remote_server_id: Option<String>,
    remote_path: Option<String>,
    view: Vec<Media>,
    books: Vec<CatalogBook>,
    collections: Vec<(String, String, String)>,
    status: String,
    providers: Option<Providers>,
    metadata: Option<auvemir_core::metadata::Services>,
    metadata_target: Option<Media>,
    metadata_results: Vec<auvemir_core::metadata::Candidate>,
    reference_searches: Vec<auvemir_core::metadata::ReferenceSong>,
    generation: u64,
    scan_cancel: Arc<AtomicBool>,
    sleep: Option<Instant>,
    recorder: Option<Recorder>,
    recording_generation: u64,
    layer_generation: u64,
    download_cancel: Option<tokio::sync::watch::Sender<bool>>,
    last_save: Instant,
    bookmark_ids: Vec<(String, u64)>,
    chapter_media_id: Option<String>,
    chapter_positions: Vec<(String, u64)>,
    chapter_request_generation: u64,
    last_remote_progress_sync: Instant,
    remote_progress_busy: bool,
    remote_progress_snapshot: Option<(String, u64)>,
    view_dirty: bool,
    playlist_filter: Option<i64>,
    artwork: crate::artwork::Cache,
    servers: Vec<auvemir_core::servers::Server>,
    services: Vec<auvemir_core::scrobble::Service>,
    active_server: Option<String>,
    listen: Option<auvemir_core::scrobble::Listen>,
    listen_queued: bool,
    last_tick: Instant,
    scrobble_busy: bool,
    next_scrobble: Instant,
    lyric_lines: Vec<auvemir_core::lyrics::Line>,
    lyrics_text: String,
    lyric_requests: std::collections::HashMap<String, u64>,
    lyric_request_counter: u64,
    lyric_offset: i64,
    art_attempts: std::collections::HashSet<String>,
    art_slots: Arc<tokio::sync::Semaphore>,
    allow_remote_art: bool,
    sync_busy: bool,
    pending_export: Option<PathBuf>,
    cast: Option<crate::casting::Handle>,
    cast_id: u64,
    cast_status: auvemir_core::cast::Status,
    cast_at: Instant,
    cast_new_listen: bool,
    cast_devices: Vec<auvemir_core::cast::Device>,
    system_transport: bool,
    transport: crate::transport::Transport,
    resume_from: Option<(String, u64)>,
    rooms: crate::rooms::Rooms,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    discord: Option<auvemir_core::discord::Presence>,
}
impl Worker {
    fn leave_room(&mut self) {
        if self.rooms.following {
            self.generation = self.generation.wrapping_add(1);
        }
        if let Some(speed) = self.rooms.leave() {
            self.audio.set_speed(speed);
        }
    }
    fn room_action(&mut self, action: &str, value: &str) -> Result<()> {
        if action == "leave" {
            self.leave_room();
            self.status = "Left the listening room.".into();
            return Ok(());
        }
        if action == "retry" {
            self.rooms.missing = None;
            return self.apply_room();
        }
        if self.cast.is_some() || self.recorder.is_some() {
            bail!("Disconnect casting and stop recording before joining a room");
        }
        if !matches!(action, "host" | "join") {
            bail!("Unknown room action");
        }
        if action == "host" && self.current.as_ref().is_some_and(|m| !m.seekable()) {
            bail!("Choose local or downloaded seekable audio to host a room");
        }
        self.leave_room();
        let generation = self.rooms.generation;
        let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
        self.rooms.cancel = Some(cancel);
        if action == "host" {
            let address = if value.trim().is_empty() {
                crate::rooms::default_address()?
            } else if let Ok(ip) = value.trim().parse::<std::net::IpAddr>() {
                std::net::SocketAddr::new(ip, 0)
            } else {
                value
                    .trim()
                    .parse()
                    .context("Enter a local IP address or IP:port")?
            };
            let playback = self
                .current
                .as_ref()
                .map(|m| auvemir_core::listen::Playback {
                    identity: auvemir_core::sync::identity(m),
                    title: m.title.clone(),
                    artist: m.artist.clone(),
                    position_ms: self.position(),
                    duration_ms: self.duration(),
                    playing: self.playing(),
                    speed: self.audio.speed,
                });
            self.status = "Starting listening room…".into();
            self.job(async move {
                tokio::select! {
                    _ = cancelled.changed() => Ok(Command::RoomWake(generation)),
                    result = auvemir_core::listen::Host::start(address,playback) => Ok(Command::RoomReady(generation,result.map(|host| crate::rooms::Connection::Host(Box::new(host))).map_err(|e| e.to_string()))),
                }
            });
        } else {
            self.persist()?;
            self.audio.pause();
            self.audio.stop_layers();
            self.layer_generation = self.layer_generation.wrapping_add(1);
            self.rooms.following = true;
            self.rooms.previous_speed = Some(self.audio.speed);
            let inbox: crate::rooms::Inbox = Arc::new(std::sync::Mutex::new(None));
            self.rooms.inbox = Some(inbox.clone());
            let invitation = value.to_owned();
            let tx = self.tx.clone();
            self.status = "Joining listening room…".into();
            self.job(async move {
                tokio::select! {
                    _ = cancelled.changed() => Ok(Command::RoomWake(generation)),
                    result = auvemir_core::listen::Guest::connect(&invitation,move |event| {
                        if let Ok(mut pending) = inbox.lock() { *pending = Some(event); }
                        let _ = tx.try_send(Command::RoomWake(generation));
                    }) => Ok(Command::RoomReady(generation,result.map(|guest| crate::rooms::Connection::Guest { _guest: guest }).map_err(|e| e.to_string()))),
                }
            });
        }
        Ok(())
    }
    fn apply_room(&mut self) -> Result<()> {
        if !self.rooms.following || self.transport.interrupted {
            return Ok(());
        }
        let Some((state, offset)) = &self.rooms.latest else {
            return Ok(());
        };
        let Some(playback) = &state.playback else {
            self.listen_tick();
            self.audio.pause();
            self.persist()?;
            self.status = "The room host stopped playback.".into();
            return Ok(());
        };
        let target = state.position_at(auvemir_core::listen::now_ms().saturating_add(*offset));
        let speed = playback.speed;
        let playing = playback.playing;
        let current_matches = self
            .current
            .as_ref()
            .is_some_and(|m| auvemir_core::sync::identity(m) == playback.identity);
        if self.rooms.loading.as_ref() == Some(&playback.identity) {
            return Ok(());
        }
        if self.rooms.missing.as_ref().is_some_and(|(id, count)| {
            id == &playback.identity && self.db.count().ok() == Some(*count)
        }) {
            return Ok(());
        }
        let identity = playback.identity.clone();
        let title = playback.title.clone();
        let artist = playback.artist.clone();
        let duration = playback.duration_ms;
        if self.audio.speed.to_bits() != speed.to_bits() {
            self.audio.set_speed(speed);
        }
        if !playing {
            self.listen_tick();
            self.audio.pause();
        }
        if current_matches && self.audio.player.is_some() {
            if self.seekable() && self.position().abs_diff(target) > 250 {
                self.rooms.loading = Some(identity);
                self.seek_to(target)?;
            } else if playing && !self.audio.playing() {
                self.activate_output()?;
                self.audio.toggle();
            }
            self.status = format!("Listening together · {title}");
            return Ok(());
        }
        if !playing {
            self.status = "The room host paused playback.".into();
            return Ok(());
        }
        if duration > 0 && target.saturating_add(250) >= duration {
            return Ok(());
        }
        let count = self.db.count()?;
        let Some(media) = self.db.shared_media(&identity)? else {
            self.audio.pause();
            self.rooms.missing = Some((identity, count));
            self.status = format!("Add matching audio to follow: {title} · {artist}");
            return Ok(());
        };
        if !media.seekable()
            && !self
                .db
                .download_path(&media.id)?
                .is_some_and(|p| Path::new(&p).is_file())
        {
            self.audio.pause();
            self.rooms.missing = Some((identity, count));
            self.status = "Download the matching track before following this room.".into();
            return Ok(());
        }
        self.persist()?;
        self.audio.stop();
        self.rooms.missing = None;
        self.queue.replace(vec![media], 0);
        self.rooms.loading = Some(identity);
        self.begin_play(Some(target))?;
        Ok(())
    }

    fn display_lyrics(&mut self, text: String) -> Result<()> {
        self.lyric_lines = auvemir_core::lyrics::parse_lrc(&text);
        self.lyrics_text = text.clone();
        let rows = self
            .lyric_lines
            .iter()
            .enumerate()
            .map(|(index, line)| LyricRow {
                index: index as i32,
                text: line.text.clone().into(),
            })
            .collect::<Vec<_>>();
        self.ui.upgrade_in_event_loop(move |u| {
            u.set_lyrics(text.into());
            u.set_lyric_rows(ModelRc::new(VecModel::from(rows)));
            u.set_active_lyric_index(-1);
        })?;
        Ok(())
    }
    fn restore_mix(&mut self, layers: Vec<(String, f32)>) -> Result<()> {
        if !layers.is_empty() {
            self.activate_output()?;
        }
        self.audio.stop_layers();
        self.layer_generation = self.layer_generation.wrapping_add(1);
        self.transport.layers_paused = false;
        let generation = self.layer_generation;
        for (id, gain) in layers {
            self.audio.layer_volume(&id, gain);
            if ["white", "pink", "brown"].contains(&id.as_str()) {
                self.audio.noise(&id)?;
            } else if let Some(media) = self.db.get(&id)? {
                self.job(async move {
                    Ok(Command::Layer(
                        generation,
                        id,
                        audio::prepare(media, 0, true).await?,
                    ))
                });
            }
        }
        Ok(())
    }
    fn activate_output(&self) -> Result<()> {
        crate::platform::activate_audio(self.recorder.as_ref().is_some_and(|r| !r.paused))
    }
    fn resume_audio(&mut self, targets: crate::transport::Targets) -> Result<()> {
        if targets.primary {
            if let Some(cast) = &self.cast {
                cast.control("play", 0.)?;
            } else if self.audio.player.is_some() {
                self.activate_output()?;
                self.audio.resume();
            } else {
                self.begin_play(None)?;
            }
        }
        if targets.layers && self.audio.has_layers() {
            self.activate_output()?;
            self.audio.resume_layers();
        }
        Ok(())
    }
    fn playing(&self) -> bool {
        if self.cast.is_some() {
            self.cast_status.playing
        } else {
            self.audio.playing()
        }
    }
    fn duration(&self) -> u64 {
        if self.cast.is_some() {
            self.current.as_ref().map(|m| m.duration_ms).unwrap_or(0)
        } else {
            self.audio.duration_ms
        }
    }
    fn position(&self) -> u64 {
        if self.cast.is_some() {
            let extra = if self.cast_status.playing {
                self.cast_at.elapsed().as_millis().min(u64::MAX as u128) as u64
            } else {
                0
            };
            let p = self.cast_status.position_ms.saturating_add(extra);
            if self.duration() > 0 {
                p.min(self.duration())
            } else {
                p
            }
        } else {
            self.audio.position()
        }
    }
    fn seekable(&self) -> bool {
        self.current.as_ref().is_some_and(|m| {
            if self.cast.is_some() {
                m.kind != MediaKind::Radio && m.duration_ms > 0
            } else {
                m.seekable()
            }
        })
    }
    fn seek_to(&mut self, position: u64) -> Result<()> {
        if let Some(cast) = &self.cast {
            cast.control("seek", position as f64)
        } else {
            self.begin_play(Some(position))
        }
    }
    fn new(base: PathBuf, tx: SyncSender<Command>, ui: crate::ui_target::Target) -> Result<Self> {
        let mut db = Store::open(base.join("auvemir.sqlite"))?;
        for path in crate::recorder::recover(&base.join("recordings")).unwrap_or_default() {
            if let Ok(media) = auvemir_core::library::read_media(&path, MediaKind::Recording) {
                db.upsert(&[media])?;
            }
        }
        let mut audio = Audio::new();
        if let Some(v) = db.setting("volume")?.and_then(|v| v.parse::<f32>().ok()) {
            audio.set_volume(v);
        }
        if let Some(v) = db.setting("speed")?.and_then(|v| v.parse::<f32>().ok()) {
            audio.set_speed(v);
        }
        let (ids, cursor) = db.load_queue()?;
        let items = ids
            .into_iter()
            .filter_map(|id| db.get(&id).ok().flatten())
            .collect();
        let mut queue = Queue::default();
        queue.replace(items, cursor);
        let saved_servers = db.setting("servers")?;
        let mut servers: Vec<auvemir_core::servers::Server> = saved_servers
            .clone()
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        if saved_servers.is_none() {
            servers.push(auvemir_core::servers::Server {
                id: "ytmusic".into(),
                name: "YouTube Music".into(),
                protocol: "invidious".into(),
                endpoint: "https://invidious.f5.si/".into(),
                username: "".into(),
            });
        }
        let services = db
            .setting("scrobblers")?
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        let active_server = db
            .setting("active-server")?
            .filter(|id| servers.iter().any(|s| &s.id == id))
            .or_else(|| {
                servers
                    .iter()
                    .find(|s| s.protocol == "invidious")
                    .map(|s| s.id.clone())
            });
        if let Some(gains) = db
            .setting("equalizer")?
            .and_then(|v| serde_json::from_str::<Vec<f32>>(&v).ok())
        {
            for (i, g) in gains.iter().enumerate() {
                audio.eq(i, *g);
            }
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let discord = db
            .setting("discord-app-id")?
            .filter(|s| !s.is_empty())
            .and_then(|id| auvemir_core::discord::Presence::new(&runtime, id).ok());
        let lyric_offset = db
            .setting("lyrics-offset")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let sort = MediaSort::from_index(
            db.setting("library-sort")?
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
        );
        let legacy_light = db.setting("appearance-light")?.as_deref() == Some("true");
        let theme_index = db
            .setting("appearance-theme")?
            .as_deref()
            .and_then(theme_index)
            .unwrap_or(if legacy_light { 4 } else { 0 });
        let reduce_motion = db.setting("motion-reduced")?.as_deref() == Some("true");
        Ok(Self {
            base,
            db,
            tx,
            ui,
            runtime,
            audio,
            queue,
            current: None,
            page: 0,
            sort,
            theme_index,
            reduce_motion,
            loading: false,
            home_recent: false,
            view_generation: 0,
            scan_generation: 0,
            scanning: false,
            scan_imported: 0,
            scan_errors: 0,
            offset: 0,
            query: String::new(),
            remote_has_more: false,
            remote_request_generation: 0,
            remote_server_id: None,
            remote_path: None,
            view: vec![],
            books: vec![],
            collections: vec![],
            status: "Import your audio to get started.".into(),
            providers: None,
            metadata: None,
            metadata_target: None,
            metadata_results: Vec::new(),
            reference_searches: Vec::new(),
            generation: 0,
            scan_cancel: Arc::new(AtomicBool::new(false)),
            sleep: None,
            recorder: None,
            recording_generation: 0,
            layer_generation: 0,
            download_cancel: None,
            last_save: Instant::now(),
            bookmark_ids: vec![],
            chapter_media_id: None,
            chapter_positions: vec![],
            chapter_request_generation: 0,
            last_remote_progress_sync: Instant::now(),
            remote_progress_busy: false,
            remote_progress_snapshot: None,
            view_dirty: true,
            playlist_filter: None,
            artwork: Default::default(),
            servers,
            services,
            active_server,
            listen: None,
            listen_queued: false,
            last_tick: Instant::now(),
            scrobble_busy: false,
            next_scrobble: Instant::now(),
            lyric_lines: Vec::new(),
            lyrics_text: String::new(),
            lyric_requests: Default::default(),
            lyric_request_counter: 0,
            lyric_offset,
            art_attempts: Default::default(),
            art_slots: Arc::new(tokio::sync::Semaphore::new(4)),
            allow_remote_art: false,
            sync_busy: false,
            pending_export: None,
            cast: None,
            cast_id: 0,
            cast_status: Default::default(),
            cast_at: Instant::now(),
            cast_new_listen: false,
            cast_devices: Vec::new(),
            system_transport: false,
            transport: Default::default(),
            resume_from: None,
            rooms: Default::default(),
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            discord,
        })
    }
    fn run(mut self, rx: Receiver<Command>) -> Result<()> {
        self.refresh()?;
        self.render()?;
        loop {
            let schedule = crate::wake::Schedule {
                audio_ui: self.playing() || self.recorder.as_ref().is_some_and(|r| !r.paused),
                casting: self.cast.is_some(),
                sleep: self.sleep,
                scrobble: (self.services.iter().any(|s| s.enabled) && !self.scrobble_busy)
                    .then_some(self.next_scrobble),
            };
            let cmd = if let Some(timeout) = schedule.delay(Instant::now()) {
                match rx.recv_timeout(timeout) {
                    Ok(c) => Some(c),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(_) => break,
                }
            } else {
                rx.recv().ok()
            };
            if let Some(Command::Shutdown) = cmd {
                self.leave_room();
                self.scan_cancel.store(true, Ordering::Relaxed);
                if let Some(c) = &self.download_cancel {
                    let _ = c.send(true);
                }
                self.persist()?;
                self.audio.stop_all();
                if let Some(r) = self.recorder.take()
                    && let Ok((path, _)) = r.stop()
                    && let Ok(m) = auvemir_core::library::read_media(&path, MediaKind::Recording)
                {
                    let _ = self.db.upsert(&[m]);
                }
                break;
            }
            if let Some(c) = cmd {
                if let Err(e) = self.handle(c) {
                    self.status = e.to_string();
                }
                self.render()?;
            }
            self.tick()?;
        }
        if let Some(cast) = self.cast.take() {
            self.runtime.block_on(cast.close());
        }
        self.runtime.shutdown_background();
        Ok(())
    }
    fn announce_now_playing(&self) {
        if !self
            .current
            .as_ref()
            .is_some_and(|m| matches!(m.kind, MediaKind::LocalMusic | MediaKind::OnlineMusic))
        {
            return;
        }
        let Some(listen) = self.listen.clone() else {
            return;
        };
        for service in self
            .services
            .iter()
            .filter(|s| s.enabled && s.protocol != "maloja")
        {
            if let Ok(secret) = crate::credentials::load(&service.id) {
                let service = service.clone();
                let listen = listen.clone();
                self.runtime.spawn(async move {
                    let _ = auvemir_core::scrobble::now_playing(&service, &secret, &listen).await;
                });
            }
        }
    }
    fn metadata_services(&mut self) -> Result<auvemir_core::metadata::Services> {
        if self.metadata.is_none() {
            self.metadata = Some(auvemir_core::metadata::Services::new()?);
        }
        Ok(self.metadata.as_ref().unwrap().clone())
    }
    fn providers(&mut self) -> Result<Providers> {
        if self.providers.is_none() {
            self.providers = Some(Providers::new()?);
        }
        Ok(self.providers.as_ref().unwrap().clone())
    }
    fn search_server_page(
        &mut self,
        offset: usize,
        server_id: Option<String>,
        webdav_path: Option<String>,
    ) -> Result<()> {
        let server = server_id
            .as_ref()
            .and_then(|id| self.servers.iter().find(|server| &server.id == id))
            .or_else(|| {
                self.servers
                    .iter()
                    .find(|s| Some(&s.id) == self.active_server.as_ref())
            })
            .or_else(|| self.servers.iter().find(|s| s.protocol == "invidious"))
            .or_else(|| self.servers.first())
            .cloned()
            .context("No online music server available")?;
        let secret = if server.protocol == "invidious" {
            String::new()
        } else {
            crate::credentials::load(&server.id)?
        };
        self.page = 2;
        self.offset = offset;
        self.remote_server_id = Some(server.id.clone());
        self.remote_path = webdav_path.clone();
        self.remote_has_more = false;
        self.remote_request_generation = self.remote_request_generation.wrapping_add(1);
        let generation = self.remote_request_generation;
        let query = self.query.clone();
        let path = webdav_path;
        self.view.clear();
        self.view_dirty = true;
        self.collections.clear();
        self.status = format!("Loading results from {}…", server.name);
        self.loading = true;
        self.job(async move {
            let result = server
                .search_directory_page(&secret, &query, offset, path.as_deref())
                .await
                .map_err(|error| error.to_string());
            Ok(Command::RemotePage(generation, offset, result))
        });
        Ok(())
    }
    fn job<F>(&self, future: F)
    where
        F: std::future::Future<Output = Result<Command>> + Send + 'static,
    {
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            let result = match future.await {
                Ok(c) => c,
                Err(e) => Command::Notice(e.to_string()),
            };
            let _ = tokio::task::spawn_blocking(move || {
                let _ = tx.send(result);
            })
            .await;
        });
    }
    fn view_job<F>(&mut self, future: F)
    where
        F: std::future::Future<Output = Result<Command>> + Send + 'static,
    {
        self.loading = true;
        let generation = self.view_generation;
        self.job(async move {
            let result = future
                .await
                .unwrap_or_else(|e| Command::Notice(e.to_string()));
            Ok(Command::ViewResult(generation, Box::new(result)))
        });
    }
    fn handle(&mut self, cmd: Command) -> Result<()> {
        if matches!(
            &cmd,
            Command::Navigate(_)
                | Command::OpenFile(_)
                | Command::Search(_, _)
                | Command::Collection(_)
                | Command::OpenPlaylist(_)
                | Command::Subscribe(_, _)
                | Command::Import(_, _)
        ) {
            self.view_generation = self.view_generation.wrapping_add(1);
            self.loading = false;
        }
        {
            let local_transport = match &cmd {
                Command::Play(_)
                | Command::SeekLyric(_, _)
                | Command::BookTracks(_, _)
                | Command::Seek(_)
                | Command::Speed(_)
                | Command::ConnectCast(_) => true,
                Command::Collection(id) if id.starts_with("chapter:") => true,
                Command::MediaControl(e) => !matches!(
                    e,
                    crate::controls::MediaControlEvent::SetVolume(_)
                        | crate::controls::MediaControlEvent::Raise
                ),
                Command::Native(a, _, _) => matches!(
                    a.as_str(),
                    "play-id"
                        | "play"
                        | "pause"
                        | "toggle"
                        | "next"
                        | "previous"
                        | "stop"
                        | "seek"
                        | "record"
                ),
                Command::Action(a) => matches!(
                    a.as_str(),
                    "toggle"
                        | "next"
                        | "previous"
                        | "stop"
                        | "back15"
                        | "forward30"
                        | "play-all"
                        | "record"
                        | "clear-queue"
                ),
                _ => false,
            };
            if local_transport && !self.system_transport {
                crate::platform::user_transport();
                self.transport.interrupted = false;
                if self.rooms.following {
                    self.leave_room();
                }
            }
        }
        if self.rooms.hosting()
            && (matches!(&cmd, Command::ConnectCast(_))
                || matches!(&cmd,Command::Action(a) | Command::Native(a,_,_) if a == "record"))
        {
            self.leave_room();
        }
        match cmd {
            Command::ViewResult(generation, result) => {
                if generation != self.view_generation {
                    // A completed subscription remains useful, without navigating away.
                    if let Command::Feed(url, title, kind, items) = *result {
                        self.db.upsert(&items)?;
                        self.db.subscription(&url, &title, kind)?;
                    }
                    return Ok(());
                }
                self.loading = false;
                return self.handle(*result);
            }
            Command::Sort(index) => {
                self.sort = MediaSort::from_index(index);
                self.db
                    .set("library-sort", &self.sort.index().to_string())?;
                self.offset = 0;
                self.refresh()?;
            }
            Command::Theme(index) => self.set_theme(index)?,
            Command::Preference(key, value) => {
                match key.as_str() {
                    "appearance-light" => self.set_theme(if value { 4 } else { 0 })?,
                    "motion-reduced" => self.reduce_motion = value,
                    _ => bail!("Unknown appearance preference"),
                }
                self.db.set(&key, &value.to_string())?;
            }
            Command::SelectServer(index) => {
                let id = self
                    .servers
                    .get(index.max(0) as usize)
                    .context("Connection is no longer available")?
                    .id
                    .clone();
                self.action(&format!("server:{id}"))?;
            }
            Command::ScanFinished(generation, result) => {
                if generation != self.scan_generation {
                    return Ok(());
                }
                self.scanning = false;
                self.status = match result {
                    Ok(()) => format!(
                        "Import complete. {} new or updated files; {} warnings.",
                        self.scan_imported, self.scan_errors
                    ),
                    Err(error) => format!("Import stopped: {error}. Completed batches were saved."),
                };
            }

            Command::AmbientPreset(action, name) => {
                match action.as_str() {
                    "save" => {
                        let layers = self
                            .audio
                            .layer_ids()
                            .into_iter()
                            .map(|id| {
                                let gain = self.audio.layer_gain(&id);
                                (id, gain)
                            })
                            .collect();
                        self.db.save_ambient_preset(&name, layers)?;
                        self.status = format!("Saved mix: {}", name.trim());
                    }
                    "restore" => {
                        let preset = self
                            .db
                            .ambient_presets()?
                            .into_iter()
                            .find(|p| p.name == name)
                            .context("This saved mix is no longer available")?;
                        for (id, _) in &preset.layers {
                            if !["white", "pink", "brown"].contains(&id.as_str()) {
                                let media = self.db.get(id)?.context("Import the missing ambient layer again before restoring this mix")?;
                                anyhow::ensure!(
                                    media.is_remote() || Path::new(&media.location).is_file(),
                                    "An ambient file is missing. Import it again before restoring this mix"
                                );
                            }
                        }
                        self.restore_mix(preset.layers)?;
                        self.status = format!("Restored mix: {name}");
                    }
                    "delete" => {
                        self.db.delete_ambient_preset(&name)?;
                        self.status = format!("Deleted saved mix: {name}");
                    }
                    _ => bail!("Unknown saved mix action"),
                }
            }
            #[cfg(target_os = "android")]
            Command::UiAttached => {
                self.display_lyrics(self.lyrics_text.clone())?;
                self.view_dirty = true;
                self.render()?;
            }
            Command::RoomAction(action, value) => {
                if let Err(error) = self.room_action(&action, &value) {
                    self.leave_room();
                    return Err(error);
                }
            }
            Command::RoomReady(g, result) => {
                if g == self.rooms.generation {
                    self.rooms.cancel = None;
                    match result {
                        Ok(connection) => {
                            if let crate::rooms::Connection::Host(host) = &connection {
                                self.rooms.invitation = host.invitation.clone();
                                self.status =
                                    "Room ready. Share the invitation with your friends.".into();
                            } else {
                                self.status = "Room connected. Waiting for the host…".into();
                            }
                            self.rooms.connection = Some(connection);
                            self.ui
                                .upgrade_in_event_loop(|u| u.set_room_link("".into()))?;
                        }
                        Err(error) => {
                            self.leave_room();
                            self.status = error;
                        }
                    }
                }
            }
            Command::RoomWake(g) => {
                let _ = g == self.rooms.generation;
            }
            Command::PrepareFailed(g, error) => {
                if g == self.generation {
                    self.rooms.loading = None;
                    if self.rooms.following {
                        self.audio.pause();
                        if let Some((state, _)) = &self.rooms.latest
                            && let Some(p) = &state.playback
                        {
                            self.rooms.missing = Some((p.identity.clone(), self.db.count()?));
                        }
                    }
                    self.status = error;
                }
            }
            Command::Navigate(p) => {
                self.remote_request_generation = self.remote_request_generation.wrapping_add(1);
                self.remote_has_more = false;
                self.remote_server_id = None;
                self.remote_path = None;
                self.allow_remote_art = true;
                self.ui.upgrade_in_event_loop(move |u| {
                    u.set_search_query("".into());
                    if p == 3 {
                        u.set_audiobook_section(0);
                    }
                })?;
                self.page = p;
                self.offset = 0;
                self.query.clear();
                self.collections.clear();
                self.playlist_filter = None;
                self.refresh()?;
                if p == 2 {
                    self.search_server_page(0, None, None)?;
                } else if p == 5 {
                    if let Ok(providers) = self.providers() {
                        self.status = "Loading popular radio stations…".into();
                        self.view_job(
                            async move { Ok(Command::Stations(providers.radio("").await?)) },
                        );
                    }
                } else if p == 3 {
                    if let Ok(providers) = self.providers() {
                        self.status = "Loading LibriVox audiobooks…".into();
                        self.view_job(
                            async move { Ok(Command::Catalog(providers.books("").await?)) },
                        );
                    }
                } else if p == 4
                    && let Ok(providers) = self.providers()
                {
                    self.status = "Loading popular podcasts…".into();
                    self.view_job(
                        async move { Ok(Command::Catalog(providers.podcasts("").await?)) },
                    );
                }
            }
            Command::Play(id) => {
                if let Some(m) = self.db.get(&id)?
                    && let Some(server) = self
                        .servers
                        .iter()
                        .find(|s| {
                            s.protocol == "audiobookshelf"
                                && m.provider == format!("server:{}", s.id)
                        })
                        .cloned()
                    && !url::Url::parse(&m.location)?
                        .query_pairs()
                        .any(|(key, _)| key == "track")
                    && self.db.download_path(&id)?.is_none()
                {
                    let secret = crate::credentials::load(&server.id)?;
                    self.status = "Loading book tracks…".into();
                    self.job(async move {
                        let (tracks, progress) =
                            server.expand_book_with_progress(&secret, &m).await?;
                        Ok(Command::BookTracks(tracks, progress))
                    });
                    return Ok(());
                }
                if self.page == 8 {
                    if let Some(i) = self.queue.items.iter().position(|m| m.id == id) {
                        self.queue.cursor = i;
                    }
                } else {
                    let i = self
                        .view
                        .iter()
                        .position(|m| m.id == id)
                        .context("Item is no longer visible")?;
                    self.queue.replace(self.view.clone(), i);
                }
                if self
                    .queue
                    .current()
                    .is_some_and(|m| m.kind == MediaKind::Ambient)
                {
                    self.transport.layers_paused = false;
                }
                self.begin_play(None)?;
            }
            Command::Prepared(g, m, offset, p) => {
                if g == self.generation {
                    self.listen_tick();
                    self.persist()?;
                    let new_listen = self.listen.as_ref().is_none_or(|l| l.media_id != m.id)
                        || self.audio.ended();
                    if new_listen {
                        self.listen = Some(auvemir_core::scrobble::Listen {
                            media_id: m.id.clone(),
                            title: m.title.clone(),
                            artist: m.artist.clone(),
                            album: m.album.clone(),
                            duration_ms: p.duration_ms.max(m.duration_ms),
                            started_at: auvemir_core::scrobble::now(),
                            listened_ms: 0,
                        });
                        self.listen_queued = false;
                    }
                    let playing = !self.transport.interrupted
                        && (!self.rooms.following
                            || self
                                .rooms
                                .latest
                                .as_ref()
                                .and_then(|(s, _)| s.playback.as_ref())
                                .is_some_and(|p| p.playing));
                    if playing {
                        self.activate_output()?;
                    }
                    if playing {
                        self.audio.play(p, offset)?;
                    } else {
                        self.audio.play_state(p, offset, false)?;
                    }
                    self.rooms.loading = None;
                    self.last_tick = Instant::now();
                    self.current = Some(m.clone());
                    if new_listen {
                        self.announce_now_playing();
                        self.display_lyrics(String::new())?;
                    }
                    self.db.played(&m.id)?;
                    if self.page == 0 {
                        self.refresh()?;
                    }
                    self.last_save = Instant::now();
                    self.status = format!("Playing {}", m.title);
                    self.persist_queue()?;
                }
            }
            Command::CaptureFinished(g) => {
                if g == self.recording_generation && self.recorder.is_some() {
                    self.action("record")?;
                }
            }
            Command::Layer(generation, id, p) => {
                if generation != self.layer_generation {
                    return Ok(());
                }
                if !self.audio.has_layer(&id) && !self.transport.layers_paused {
                    self.activate_output()?;
                }
                self.audio.layer(id, p, !self.transport.layers_paused)?;
                self.status = "Ambient layer toggled. Imported loops can play together.".into();
            }
            Command::Import(path, page) => {
                if path.trim().is_empty() {
                    return self.action("pick-files");
                }
                self.scan_cancel.store(true, Ordering::Relaxed);
                self.scan_cancel = Arc::new(AtomicBool::new(false));
                self.scan_generation = self.scan_generation.wrapping_add(1);
                let scan_generation = self.scan_generation;
                self.scanning = true;
                self.scan_imported = 0;
                self.scan_errors = 0;
                let cancel = self.scan_cancel.clone();
                let kind = kind_for_page(page).unwrap_or(MediaKind::LocalMusic);
                let known = if kind == MediaKind::LocalMusic {
                    self.db.fingerprints()?
                } else {
                    std::collections::HashMap::new()
                };
                let path = PathBuf::from(path);
                self.page = page;
                self.offset = 0;
                self.query.clear();
                self.collections.clear();
                self.playlist_filter = None;
                self.refresh()?;
                self.status = "Scanning audio in the background…".into();
                let tx = self.tx.clone();
                let artwork = self.base.join("artwork");
                self.job(async move {
                    let result = tokio::task::spawn_blocking(move || {
                        auvemir_core::library::scan_batched(
                            &path,
                            kind,
                            &known,
                            cancel,
                            Some(&artwork),
                            |batch| {
                                tx.send(Command::Scanned(scan_generation, batch))
                                    .map_err(|_| anyhow::anyhow!("Library worker stopped"))
                            },
                        )
                    })
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.map_err(|e| e.to_string()));
                    Ok(Command::ScanFinished(scan_generation, result))
                });
            }
            Command::OpenFile(path) => {
                let path = PathBuf::from(path).canonicalize()?;
                let m = auvemir_core::library::read_media(&path, MediaKind::LocalMusic)?;
                self.db.upsert(std::slice::from_ref(&m))?;
                self.page = 1;
                self.refresh()?;
                self.queue.replace(vec![m], 0);
                self.begin_play(Some(0))?;
            }
            Command::Scanned(generation, scan) => {
                if generation != self.scan_generation || !self.scanning {
                    return Ok(());
                }
                if let Err(error) = self.db.save_scan(&scan) {
                    self.scan_cancel.store(true, Ordering::Relaxed);
                    self.scan_generation = self.scan_generation.wrapping_add(1);
                    self.scanning = false;
                    return Err(error);
                }
                self.rooms.missing = None;
                self.scan_imported += scan.items.len();
                self.scan_errors += scan.errors.len();
                self.status = format!(
                    "Importing… {} new or updated files; {} warnings.",
                    self.scan_imported, self.scan_errors
                );
                self.refresh()?;
            }
            Command::Search(query, page) => {
                self.query = query.clone();
                self.offset = 0;
                self.remote_request_generation = self.remote_request_generation.wrapping_add(1);
                self.remote_has_more = false;
                self.remote_server_id = None;
                self.remote_path = None;
                match page {
                    2 => {
                        self.search_server_page(0, None, None)?;
                    }
                    5 => {
                        let p = self.providers()?;
                        self.status = "Searching Radio Browser…".into();
                        self.view_job(async move { Ok(Command::Stations(p.radio(&query).await?)) });
                    }
                    4 => {
                        let p = self.providers()?;
                        self.status = "Searching podcasts…".into();
                        self.view_job(
                            async move { Ok(Command::Catalog(p.podcasts(&query).await?)) },
                        );
                    }
                    3 => {
                        let p = self.providers()?;
                        self.status = "Searching LibriVox…".into();
                        self.view_job(async move { Ok(Command::Catalog(p.books(&query).await?)) });
                    }
                    _ => {
                        self.page = 9;
                        self.refresh()?;
                    }
                }
            }
            Command::Catalog(books) => {
                self.view.clear();
                self.allow_remote_art = true;
                self.books = books;
                self.collections = self
                    .books
                    .iter()
                    .map(|b| {
                        let sub = if b.language == "Podcast" {
                            format!("{} · Podcast", b.author)
                        } else {
                            format!("{} · {} · LibriVox", b.author, b.language)
                        };
                        (format!("book:{}", b.id), b.title.clone(), sub)
                    })
                    .collect();
                self.view_dirty = true;
                self.status = format!(
                    "{} catalog results. Select an item to load its audio.",
                    self.books.len()
                );
            }
            Command::Collections(cols) => {
                self.view.clear();
                self.collections = cols;
                self.view_dirty = true;
                self.status = format!("{} items. Select one to browse.", self.collections.len());
            }
            Command::Stations(stations) => {
                self.collections.clear();
                self.allow_remote_art = true;
                for s in &stations {
                    self.db.save_station(s)?;
                }
                self.page = 5;
                self.view_dirty = true;
                self.view = stations
                    .into_iter()
                    .map(|s| self.db.get(&s.id)?.context("Station was not saved"))
                    .collect::<Result<Vec<_>>>()?;
                self.status =
                    "Radio directory loaded. Play a station or use + Station for your own stream."
                        .into();
            }
            Command::Subscribe(url, page) => {
                auvemir_core::network::validate_url(&url, false)?;
                if page == 2 {
                    let u = url::Url::parse(&url)?;
                    let title = u
                        .path_segments()
                        .and_then(|mut s| s.next_back())
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Online stream");
                    let m = Media::new(MediaKind::OnlineMusic, "direct-url", &url, title);
                    self.db.upsert(std::slice::from_ref(&m))?;
                    self.page = 2;
                    self.refresh()?;
                    self.queue.replace(vec![m], 0);
                    self.begin_play(Some(0))?;
                } else {
                    let kind = if page == 3 {
                        MediaKind::Audiobook
                    } else {
                        MediaKind::Podcast
                    };
                    let p = self.providers()?;
                    self.status = "Loading audio feed…".into();
                    self.view_job(async move {
                        let (title, items) = p.feed(&url, kind).await?;
                        Ok(Command::Feed(url, title, kind, items))
                    });
                }
            }
            Command::Feed(url, title, kind, items) => {
                self.allow_remote_art = true;
                self.db.upsert(&items)?;
                self.db.subscription(&url, &title, kind)?;
                self.page = if kind == MediaKind::Audiobook { 3 } else { 4 };
                self.collections.clear();
                self.refresh()?;
                self.status = format!(
                    "Added {} audio chapters / episodes from {title}",
                    items.len()
                );
            }
            Command::SaveStation(mut s) => {
                // Preserve custom/public identity when editing an existing local record.
                if let Some(old) = self.db.stations()?.into_iter().find(|o| o.id == s.id) {
                    s.custom = old.custom;
                    s.favorite = old.favorite;
                }
                self.db.save_station(&s)?;
                self.page = 5;
                self.query.clear();
                self.refresh()?;
                self.ui
                    .upgrade_in_event_loop(|u| u.set_radio_editor(false))?;
                self.status = "Station saved to your collection.".into();
            }
            Command::DeleteStation(id) => {
                self.db.delete_station(&id)?;
                self.refresh()?;
                self.ui
                    .upgrade_in_event_loop(|u| u.set_radio_editor(false))?;
                self.status = "Custom station deleted.".into();
            }
            Command::EditStation(id) => {
                let s = self
                    .db
                    .stations()?
                    .into_iter()
                    .find(|s| s.id == id)
                    .context("Station is no longer available")?;
                self.ui.upgrade_in_event_loop(move |u| {
                    u.set_station_id(s.id.into());
                    u.set_station_name(s.name.into());
                    u.set_station_url(s.url.into());
                    u.set_station_country(s.country.into());
                    u.set_station_language(s.language.into());
                    u.set_station_tags(s.tags.into());
                    u.set_station_logo(s.logo.unwrap_or_default().into());
                })?;
            }
            Command::Favorite(id) => {
                self.db.favorite(&id)?;
                if let Some(m) = &mut self.current
                    && m.id == id
                {
                    m.favorite = !m.favorite;
                }
                for m in &mut self.queue.items {
                    if m.id == id {
                        m.favorite = !m.favorite;
                    }
                }
                self.refresh()?;
            }
            Command::Seek(f) => {
                if let Some(m) = &self.current {
                    if !m.seekable() {
                        bail!("This source does not support seeking");
                    }
                    let target = (self.duration() as f32 * f.clamp(0.0, 1.0)) as u64;
                    self.seek_to(target)?;
                }
            }
            Command::Volume(v) => {
                if let Some(cast) = &self.cast {
                    cast.control("volume", (v / 100.) as f64)?;
                }
                self.audio.set_volume(v / 100.0);
                self.db.set("volume", &self.audio.volume.to_string())?;
            }
            Command::Speed(v) => {
                if self.cast.is_some() {
                    bail!("Playback speed is controlled by the Cast receiver");
                }
                self.audio.set_speed(v);
                self.db.set("speed", &self.audio.speed.to_string())?;
            }
            Command::Eq(index, gain) => {
                if self.cast.is_some() {
                    bail!("Equalization applies to local playback");
                }
                if index >= 0 {
                    self.audio.eq(index as usize, gain);
                    self.db.set(
                        "equalizer",
                        &serde_json::to_string(&self.audio.dsp.gains())?,
                    )?;
                }
            }
            Command::LayerVolume(id, v) => {
                self.audio.layer_volume(&id, v / 100.);
            }
            Command::SavePlaylist(name) => {
                self.db.playlist(
                    &name,
                    &self
                        .queue
                        .items
                        .iter()
                        .map(|m| m.id.clone())
                        .collect::<Vec<_>>(),
                )?;
                self.status = "Playlist saved.".into();
            }
            Command::PlaylistAction(action, id, name) => {
                let id = id.parse()?;
                match action.as_str() {
                    "rename" => self.db.rename_playlist(id, &name)?,
                    "delete" => self.db.delete_playlist(id)?,
                    _ => bail!("Unknown playlist action"),
                };
                self.status = "Playlist updated.".into();
            }
            Command::OpenPlaylist(id) => {
                let id = id.parse()?;
                self.page = 1;
                self.playlist_filter = Some(id);
                self.refresh()?;
            }
            Command::Collection(id) => {
                if id.starts_with("webdav-dir:") {
                    let (server_id, path) = parse_webdav_directory_id(&id)?;
                    self.search_server_page(0, Some(server_id), Some(path))?;
                } else if let Some(index) = id.strip_prefix("metadata:") {
                    let candidate = self
                        .metadata_results
                        .get(index.parse::<usize>()?)
                        .cloned()
                        .context("Metadata result is no longer available")?;
                    let m = self
                        .metadata_target
                        .clone()
                        .context("No metadata target is selected")?;
                    self.db.set(
                        &format!("metadata-refs:{}", m.id),
                        &serde_json::to_string(&candidate)?,
                    )?;
                    if !candidate.url.is_empty() {
                        crate::platform::open_url(&candidate.url)?;
                        self.status = "Opened the provider page.".into();
                    } else {
                        if m.is_remote() || m.provider != "local" {
                            bail!("Tag editing requires a local audio file");
                        }
                        self.job(async move {
                            let media = m.clone();
                            let mut tags = tokio::task::spawn_blocking(move || {
                                auvemir_core::library::read_tags(Path::new(&media.location))
                            })
                            .await??;
                            if !candidate.title.is_empty() {
                                tags.title = candidate.title;
                            }
                            if !candidate.artist.is_empty() {
                                tags.artist = candidate.artist;
                            }
                            if !candidate.album.is_empty() {
                                tags.album = candidate.album;
                            }
                            Ok(Command::TagDraft(m, tags))
                        });
                    }
                } else if let Some(reference) = id.strip_prefix("reference:") {
                    self.reference_searches = self.db.reference_missing(reference)?;
                    self.collections = self
                        .reference_searches
                        .iter()
                        .enumerate()
                        .map(|(i, t)| (format!("ref-song:{i}"), t.title.clone(), t.artist.clone()))
                        .collect();
                    self.view.clear();
                    self.view_dirty = true;
                    self.status =
                        "Choose a missing track to search your online music connection.".into();
                } else if let Some(index) = id.strip_prefix("ref-song:") {
                    let track = self
                        .reference_searches
                        .get(index.parse::<usize>()?)
                        .context("Imported reference is no longer selected")?;
                    self.handle(Command::Search(
                        format!("{} {}", track.title, track.artist),
                        2,
                    ))?;
                    self.page = 2;
                } else if let Some(address) = id.strip_prefix("cast:") {
                    self.handle(Command::ConnectCast(address.into()))?;
                } else if let Some(group) = id.strip_prefix("group:") {
                    let (field, value) = group.split_once(':').context("Invalid collection")?;
                    let value = url::form_urlencoded::parse(format!("v={value}").as_bytes())
                        .next()
                        .map(|(_, v)| v.into_owned())
                        .unwrap_or_default();
                    let kind = if self.page == 3 {
                        MediaKind::Audiobook
                    } else {
                        MediaKind::LocalMusic
                    };
                    self.view = self.db.group_items(kind, field, &value)?;
                    self.view_dirty = true;
                    self.collections.clear();
                    self.status = value;
                } else if let Some(url) = id.strip_prefix("feed:") {
                    self.handle(Command::Subscribe(url.into(), 4))?;
                } else if let Some(country) = id.strip_prefix("radio-country:") {
                    let country = country.to_string();
                    let p = self.providers()?;
                    self.status = format!("Loading radio stations for {country}…");
                    self.collections.clear();
                    self.view_job(async move {
                        Ok(Command::Stations(
                            p.radio(&format!("country:{country}")).await?,
                        ))
                    });
                } else if let Some(tag) = id.strip_prefix("radio-genre:") {
                    let tag = tag.to_string();
                    let p = self.providers()?;
                    self.status = format!("Loading radio stations for genre {tag}…");
                    self.collections.clear();
                    self.view_job(async move {
                        Ok(Command::Stations(p.radio(&format!("tag:{tag}")).await?))
                    });
                } else if let Some(genre_id) = id.strip_prefix("book-genre:") {
                    let genre_id = genre_id.to_string();
                    let p = self.providers()?;
                    self.status = "Loading audiobooks for genre…".into();
                    self.collections.clear();
                    self.view_job(async move {
                        Ok(Command::Catalog(
                            p.books(&format!("genre:{genre_id}")).await?,
                        ))
                    });
                } else if let Some(genre) = id.strip_prefix("podcast-genre:") {
                    let genre = genre.to_string();
                    let p = self.providers()?;
                    self.status = format!("Loading podcasts for {genre}…");
                    self.collections.clear();
                    self.view_job(async move { Ok(Command::Catalog(p.podcasts(&genre).await?)) });
                } else if let Some(id) = id.strip_prefix("book:") {
                    let b = self
                        .books
                        .iter()
                        .find(|b| b.id == id)
                        .context("Book is no longer available")?
                        .clone();
                    let p = self.providers()?;
                    let kind = if b.language == "Podcast" {
                        MediaKind::Podcast
                    } else {
                        MediaKind::Audiobook
                    };
                    self.status = format!("Loading {}…", b.title);
                    self.view_job(async move {
                        let (title, mut items) = p.feed(&b.feed_url, kind).await?;
                        for m in &mut items {
                            m.artist = b.author.clone();
                            m.album = b.title.clone();
                            m.artwork = b.cover.clone();
                        }
                        Ok(Command::Feed(b.feed_url, title, kind, items))
                    });
                } else if let Some(position) = self
                    .chapter_positions
                    .iter()
                    .find(|(key, _)| key == &id)
                    .map(|(_, position)| *position)
                {
                    anyhow::ensure!(
                        self.chapter_media_id.as_deref()
                            == self.current.as_ref().map(|m| m.id.as_str()),
                        "The playing audiobook changed"
                    );
                    anyhow::ensure!(self.seekable(), "This audiobook cannot seek");
                    self.seek_to(position)?;
                    self.status = "Jumped to audiobook chapter.".into();
                } else if let Some((_, position)) =
                    self.bookmark_ids.iter().find(|(key, _)| key == &id)
                {
                    self.begin_play(Some(*position))?;
                }
            }
            Command::Bookmark(name) => {
                let m = self
                    .current
                    .as_ref()
                    .context("Play a local audiobook or podcast first")?;
                if !m.seekable() {
                    bail!("Download this audio before adding precise bookmarks");
                }
                self.db.bookmark(&m.id, self.position(), &name)?;
                self.status = "Bookmark saved.".into();
            }
            Command::Sleep(minutes) => {
                let min: u64 = minutes
                    .trim()
                    .parse()
                    .context("Enter a number of minutes")?;
                if min > 1440 {
                    bail!("Sleep timer maximum is 1440 minutes");
                }
                self.sleep = if min == 0 {
                    None
                } else {
                    Some(Instant::now() + Duration::from_secs(min * 60))
                };
                self.status = if min == 0 {
                    "Sleep timer off".into()
                } else {
                    format!("Playback stops in {min} minutes")
                };
            }
            Command::Lyrics(id, text) => {
                anyhow::ensure!(text.len() <= 2 * 1024 * 1024, "Lyrics exceed 2 MiB");
                self.db.save_lyrics(&id, &text)?;
                self.lyric_requests.remove(&id);
                if self.current.as_ref().is_some_and(|m| m.id == id) {
                    self.display_lyrics(text)?;
                    self.status = "Lyrics cached for offline use.".into();
                }
            }
            Command::LyricsFetched(id, request, result) => {
                if self.lyric_requests.get(&id) != Some(&request) {
                    return Ok(());
                }
                self.lyric_requests.remove(&id);
                match result {
                    Ok(text) => self.handle(Command::Lyrics(id, text))?,
                    Err(error) if self.current.as_ref().is_some_and(|m| m.id == id) => {
                        self.status = error
                    }
                    Err(_) => {}
                }
            }
            Command::EmbeddedChapters(id, request, result) => {
                if request != self.chapter_request_generation
                    || !self.current.as_ref().is_some_and(|m| m.id == id)
                {
                    return Ok(());
                }
                match result {
                    Err(error) => {
                        self.status = format!("Could not read embedded chapters: {error}");
                    }
                    Ok(chapters) => {
                        let duration = self.duration();
                        let mut rows = Vec::with_capacity(chapters.len());
                        let mut positions = Vec::with_capacity(chapters.len());
                        for (index, chapter) in chapters.into_iter().enumerate() {
                            if duration > 0 && chapter.position_ms >= duration {
                                continue;
                            }
                            let title = if chapter.title.trim().is_empty() {
                                format!("Chapter {}", index + 1)
                            } else {
                                chapter.title
                            };
                            let row_id = format!("chapter:{index}");
                            rows.push((row_id.clone(), title, format_time(chapter.position_ms)));
                            positions.push((row_id, chapter.position_ms));
                        }
                        self.collections = rows;
                        self.chapter_positions = positions;
                        self.chapter_media_id = (!self.chapter_positions.is_empty()).then_some(id);
                        self.status = if self.chapter_positions.is_empty() {
                            "No playable embedded chapters were found.".into()
                        } else {
                            format!(
                                "{} embedded chapters. Choose one to jump to it.",
                                self.chapter_positions.len()
                            )
                        };
                    }
                }
            }
            Command::SaveLyrics(id, text) => {
                anyhow::ensure!(text.len() <= 2 * 1024 * 1024, "Lyrics exceed 2 MiB");
                self.db
                    .get(&id)?
                    .context("The track being edited is no longer available")?;
                self.db.save_lyrics(&id, &text)?;
                self.lyric_requests.remove(&id);
                if self.current.as_ref().is_some_and(|m| m.id == id) {
                    self.display_lyrics(text)?;
                }
                self.ui.upgrade_in_event_loop(move |u| {
                    if u.get_lyrics_edit_id() == id {
                        u.set_lyrics_editor(false);
                    }
                })?;
                self.status = "Lyrics saved locally.".into();
            }
            Command::SeekLyric(id, index) => {
                anyhow::ensure!(
                    self.current.as_ref().is_some_and(|m| m.id == id),
                    "The playing track changed"
                );
                anyhow::ensure!(self.seekable(), "This audio cannot seek");
                let line = usize::try_from(index)
                    .ok()
                    .and_then(|i| self.lyric_lines.get(i))
                    .context("This timed line is no longer available")?;
                self.seek_to(auvemir_core::lyrics::seek_position(
                    line.ms,
                    self.lyric_offset,
                    self.duration(),
                ))?;
            }
            Command::Downloaded(id, path) => {
                self.db
                    .attach_download(&id, path.to_str().context("Download path is not UTF-8")?)?;
                self.rooms.missing = None;
                self.download_cancel = None;
                self.refresh()?;
                self.status = "Download complete. Listening progress was preserved.".into();
            }
            Command::DownloadFailed(error) => {
                self.download_cancel = None;
                self.status = error;
            }
            Command::Files(action, path) => self.files_action(&action, &path)?,
            Command::Configure(protocol, name, endpoint, username, secret) => {
                let id = stable_id("connection", &format!("{protocol}:{endpoint}:{username}"));
                if ["subsonic", "audiobookshelf", "webdav", "invidious"]
                    .contains(&protocol.as_str())
                {
                    let server = auvemir_core::servers::Server {
                        id: id.clone(),
                        name,
                        protocol,
                        endpoint,
                        username,
                    };
                    server.validate()?;
                    if server.protocol != "invidious" {
                        crate::credentials::save(&id, &secret)?;
                    }
                    self.servers.retain(|s| s.id != id);
                    self.servers.push(server);
                    self.active_server = Some(id.clone());
                    self.db
                        .set("servers", &serde_json::to_string(&self.servers)?)?;
                    self.db.set("active-server", &id)?;
                } else {
                    let service = auvemir_core::scrobble::Service {
                        id: id.clone(),
                        name,
                        protocol,
                        endpoint,
                        enabled: true,
                    };
                    service.validate()?;
                    crate::credentials::save(&id, &secret)?;
                    self.services.retain(|s| s.id != id);
                    self.services.push(service);
                    self.db
                        .set("scrobblers", &serde_json::to_string(&self.services)?)?;
                }
                self.ui
                    .upgrade_in_event_loop(|u| u.set_service_secret("".into()))?;
                self.status =
                    "Connection saved. Secrets are stored by the operating system.".into();
            }
            Command::RemotePage(generation, offset, result) => {
                if generation != self.remote_request_generation
                    || offset != self.offset
                    || self.page != 2
                {
                    return Ok(());
                }
                self.loading = false;
                let page = match result {
                    Ok(result) => result,
                    Err(error) => {
                        self.status = error;
                        return Ok(());
                    }
                };
                self.allow_remote_art = true;
                self.remote_has_more = page.has_more;
                self.db.upsert(&page.items)?;
                self.page = 2;
                self.view = page
                    .items
                    .into_iter()
                    .map(|m| self.db.get(&m.id)?.context("Server item was not saved"))
                    .collect::<Result<Vec<_>>>()?;
                self.collections.clear();
                if let (Some(server_id), Some(path)) = (
                    self.remote_server_id.as_deref(),
                    self.remote_path.as_deref(),
                ) && let Some(parent) = webdav_parent_path(path)
                {
                    self.collections.push((
                        webdav_directory_id(server_id, &parent),
                        "Parent directory".into(),
                        path.to_owned(),
                    ));
                }
                if let Some(server_id) = self.remote_server_id.as_deref() {
                    self.collections
                        .extend(page.folders.into_iter().map(|folder| {
                            (
                                webdav_directory_id(server_id, &folder.path),
                                folder.name,
                                "WebDAV folder".into(),
                            )
                        }));
                }
                self.view_dirty = true;
                self.status = format!("{} results from your server.", self.view.len());
            }
            Command::BookTracks(items, progress) => {
                self.db.upsert(&items)?;
                self.page = 3;
                self.view = items.clone();
                self.view_dirty = true;
                self.collections.clear();
                let (cursor, track_offset) = audiobook_resume_offset(&items, progress);
                self.queue.replace(items, cursor);
                self.begin_play(Some(track_offset))?;
            }
            Command::Scrobbled(results) => {
                for (id, success) in results {
                    self.db
                        .scrobble_result(id, success, auvemir_core::scrobble::now())?;
                }
                self.scrobble_busy = false;
                self.next_scrobble = Instant::now() + Duration::from_secs(30);
            }
            Command::RemoteProgressSynced(item_id, result) => {
                self.remote_progress_busy = false;
                if self
                    .current
                    .as_ref()
                    .is_some_and(|media| media.group_id == item_id)
                    && let Err(error) = result
                {
                    self.status = format!("Audiobookshelf progress sync failed: {error}");
                }
            }
            Command::MediaControl(event) => {
                use crate::controls::{MediaControlEvent as E, SeekDirection};
                self.listen_tick();
                match event {
                    E::Play => {
                        let targets = self.transport.play(crate::transport::Targets {
                            primary: !self.audio.has_layers() || self.playing(),
                            layers: self.audio.has_layers(),
                        });
                        self.resume_audio(targets)?;
                    }
                    E::Pause => {
                        self.transport.pause(crate::transport::Targets {
                            primary: self.playing(),
                            layers: self.audio.layers_playing(),
                        });
                        if let Some(cast) = &self.cast {
                            cast.control("pause", 0.)?;
                        }
                        self.audio.pause();
                        self.audio.pause_layers();
                        self.persist()?;
                    }
                    E::Toggle => self.handle(Command::MediaControl(
                        if self.playing() || self.audio.layers_playing() {
                            E::Pause
                        } else {
                            E::Play
                        },
                    ))?,
                    E::Next => self.action("next")?,
                    E::Previous => self.action("previous")?,
                    E::Stop => self.action("stop")?,
                    E::SetVolume(v) => self.handle(Command::Volume((v * 100.) as f32))?,
                    E::SetPosition(p) => {
                        if self.seekable() {
                            self.seek_to(p.0.as_millis() as u64)?;
                        }
                    }
                    E::Seek(d) => self.action(if d == SeekDirection::Forward {
                        "forward30"
                    } else {
                        "back15"
                    })?,
                    E::SeekBy(d, t) => {
                        if self.seekable() {
                            let pos = self.position();
                            let delta = t.as_millis() as u64;
                            self.seek_to(if d == SeekDirection::Forward {
                                pos.saturating_add(delta)
                            } else {
                                pos.saturating_sub(delta)
                            })?;
                        }
                    }
                    E::OpenUri(uri) => {
                        if uri.starts_with("http") {
                            self.handle(Command::Subscribe(uri, 2))?;
                        } else if let Ok(url) = url::Url::parse(&uri)
                            && let Ok(path) = url.to_file_path()
                        {
                            self.handle(Command::Import(path.to_string_lossy().into(), 1))?;
                        }
                    }
                    E::Quit => {
                        self.ui.upgrade_in_event_loop(|_| {
                            let _ = slint::quit_event_loop();
                        })?;
                    }
                    E::Raise => {
                        self.ui.upgrade_in_event_loop(|u| {
                            u.show().ok();
                            #[cfg(not(any(target_os = "android", target_os = "ios")))]
                            {
                                use slint::winit_030::WinitWindowAccessor;
                                u.window().with_winit_window(|w| w.focus_window());
                            }
                        })?;
                    }
                }
            }
            Command::Action(a) => self.action(&a)?,
            Command::Notice(message) => {
                if message == "Library scan completed." {
                    self.db.reconcile_references()?;
                }
                self.status = message;
                if self.status.starts_with("Download paused") {
                    self.download_cancel = None;
                }
            }
            Command::ConfigureSync(endpoint, username, password, passphrase) => {
                if !endpoint.trim().is_empty() {
                    auvemir_core::network::validate_url(&endpoint, true)?;
                }
                if passphrase.chars().count() < 12 {
                    bail!("Use a sync passphrase of at least 12 characters");
                }
                let config = auvemir_core::sync::WebDav { endpoint, username };
                crate::credentials::save(
                    "sync-webdav",
                    &serde_json::to_string(&auvemir_core::sync::Secret {
                        password,
                        passphrase,
                    })?,
                )?;
                self.db
                    .set("sync-config", &serde_json::to_string(&config)?)?;
                self.ui.upgrade_in_event_loop(|u| {
                    u.set_sync_password("".into());
                    u.set_sync_passphrase("".into());
                })?;
                self.status =
                    "Encrypted sync configured. Use the same passphrase on each device.".into();
            }
            Command::SyncFinished(result) => {
                self.sync_busy = false;
                let snapshot = match result {
                    Ok(s) => s,
                    Err(e) => {
                        self.pending_export = None;
                        return Err(anyhow::Error::msg(e));
                    }
                };
                self.db.sync_merge(&snapshot)?;
                if let Some(v) = self.db.setting("volume")?.and_then(|v| v.parse().ok()) {
                    self.audio.set_volume(v);
                }
                if let Some(v) = self.db.setting("speed")?.and_then(|v| v.parse().ok()) {
                    self.audio.set_speed(v);
                }
                if let Some(g) = self
                    .db
                    .setting("equalizer")?
                    .and_then(|v| serde_json::from_str::<Vec<f32>>(&v).ok())
                {
                    for (i, g) in g.into_iter().enumerate() {
                        self.audio.eq(i, g);
                    }
                }
                if let Some(current) = self.current.as_mut()
                    && let Some(m) = self.db.get(&current.id)?
                {
                    current.favorite = m.favorite;
                }
                self.refresh()?;
                self.status="Sync completed. Favorites, playlists, progress and playback settings are merged.".into();
                if let Some(path) = self.pending_export.take() {
                    crate::platform::export_document(&path.to_string_lossy(), "Auvemir.sync")?;
                }
            }
            Command::ConfigureApi(kind, secret) => {
                if !["genius", "acoustid", "fanart", "spotify", "youtube"].contains(&kind.as_str())
                {
                    bail!("Unknown API credential type");
                }
                if secret.is_empty() {
                    crate::credentials::remove(&format!("api-{kind}"))?;
                    self.status = format!("{kind} credential removed.");
                } else {
                    crate::credentials::save(&format!("api-{kind}"), &secret)?;
                    self.status = format!("{kind} credential saved securely.");
                }
                self.ui
                    .upgrade_in_event_loop(|u| u.set_api_secret("".into()))?;
            }
            Command::Metadata(target, results) => {
                self.collections = results
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        (
                            format!("metadata:{i}"),
                            r.title.clone(),
                            format!("{} · {}", r.artist, r.album),
                        )
                    })
                    .collect();
                self.status = format!(
                    "{} matches for {}. Choose a result to review.",
                    results.len(),
                    target.title
                );
                self.metadata_target = Some(target);
                self.metadata_results = results;
                self.view.clear();
                self.view_dirty = true;
                self.page = 9;
            }
            Command::ImportReference(provider, url) => {
                let services = self.metadata_services()?;
                let key = if matches!(provider.as_str(), "spotify" | "youtube") {
                    Some(crate::credentials::load(&format!("api-{provider}"))?)
                } else {
                    None
                };
                self.status = "Importing playlist metadata…".into();
                self.job(async move {
                    let playlist = match provider.as_str() {
                        "spotify" => services.spotify(&url, key.as_deref().unwrap_or("")).await?,
                        "deezer" => services.deezer(&url).await?,
                        "youtube" => services.youtube(&url, key.as_deref().unwrap_or("")).await?,
                        _ => bail!("Choose Spotify, Deezer or YouTube Music"),
                    };
                    Ok(Command::ReferenceLoaded(playlist))
                });
            }
            Command::ReferenceLoaded(playlist) => {
                let report = self.db.import_reference(&playlist)?;
                self.page = 1;
                self.playlist_filter = Some(report.playlist_id);
                self.collections.clear();
                self.refresh()?;
                self.status = format!(
                    "Imported {}: {} of {} tracks matched. Unmatched metadata is retained; use Imported playlists to search for missing audio.",
                    playlist.name, report.matched, report.total
                );
            }
            Command::ConfigureDiscord(id) => {
                #[cfg(not(any(target_os = "android", target_os = "ios")))]
                {
                    self.discord = Some(auvemir_core::discord::Presence::new(
                        &self.runtime,
                        id.clone(),
                    )?);
                    self.db.set("discord-app-id", &id)?;
                    self.status = "Discord presence enabled for music playback.".into();
                }
                #[cfg(any(target_os = "android", target_os = "ios"))]
                {
                    let _ = id;
                    bail!("Discord IPC is available on desktop platforms");
                }
            }
            Command::TagDraft(m, tags) => {
                self.ui.upgrade_in_event_loop(move |u| {
                    u.set_tag_id(m.id.into());
                    u.set_tag_title(tags.title.into());
                    u.set_tag_artist(tags.artist.into());
                    u.set_tag_album(tags.album.into());
                    u.set_tag_genre(tags.genre.into());
                    u.set_tag_composer(tags.composer.into());
                    u.set_tag_editor(true);
                })?;
            }
            Command::SaveTags(id, tags) => {
                let m = self
                    .db
                    .get(&id)?
                    .context("This audio is no longer in the library")?;
                if m.is_remote() || m.provider != "local" {
                    bail!("Tag editing requires a local audio file");
                }
                self.status = "Saving tags…".into();
                self.job(async move {
                    Ok(Command::Tagged(
                        tokio::task::spawn_blocking(move || -> Result<Media> {
                            let path = Path::new(&m.location);
                            auvemir_core::library::write_tags(path, &tags)?;
                            let mut updated = auvemir_core::library::read_media(path, m.kind)?;
                            updated.favorite = m.favorite;
                            updated.artwork = m.artwork;
                            Ok(updated)
                        })
                        .await??,
                    ))
                });
            }
            Command::ArtSelected(m) => {
                self.db.upsert(std::slice::from_ref(&m))?;
                if self.current.as_ref().is_some_and(|old| old.id == m.id) {
                    self.current = Some(m.clone());
                }
                for item in &mut self.queue.items {
                    if item.id == m.id {
                        *item = m.clone();
                    }
                }
                self.allow_remote_art = true;
                self.refresh()?;
                self.status = "Artwork selected.".into();
            }
            Command::Tagged(m) => {
                self.rooms.missing = None;
                self.db.upsert(std::slice::from_ref(&m))?;
                if self.current.as_ref().is_some_and(|old| old.id == m.id) {
                    self.current = Some(m.clone());
                }
                for item in &mut self.queue.items {
                    if item.id == m.id {
                        *item = m.clone();
                    }
                }
                self.ui.upgrade_in_event_loop(|u| u.set_tag_editor(false))?;
                self.refresh()?;
                self.status = "Tags saved.".into();
            }
            Command::CastDevices(devices) => {
                self.collections = devices
                    .iter()
                    .map(|d| {
                        (
                            format!("cast:{}", d.address),
                            d.name.clone(),
                            d.address.to_string(),
                        )
                    })
                    .collect();
                self.cast_devices = devices;
                self.view.clear();
                self.view_dirty = true;
                self.status = if self.cast_devices.is_empty() {
                    "No Cast receivers found. You can enter a private receiver address.".into()
                } else {
                    "Choose a Cast receiver on your network.".into()
                };
            }
            Command::ConnectCast(address) => {
                let address = address
                    .trim()
                    .parse::<std::net::IpAddr>()
                    .map(|ip| std::net::SocketAddr::new(ip, 8009))
                    .or_else(|_| address.trim().parse::<std::net::SocketAddr>())
                    .context("Enter a receiver IP address, optionally with a port")?;
                self.persist()?;
                let offset = self.position();
                self.audio.stop();
                if let Some(cast) = self.cast.take() {
                    self.runtime.block_on(cast.close());
                }
                self.cast_id = self.cast_id.wrapping_add(1);
                let id = self.cast_id;
                let tx = self.tx.clone();
                let device = self
                    .cast_devices
                    .iter()
                    .find(|d| d.address == address)
                    .cloned()
                    .unwrap_or(auvemir_core::cast::Device {
                        name: address.to_string(),
                        address,
                    });
                self.cast = Some(crate::casting::Handle::start(
                    &self.runtime,
                    device,
                    move |event| {
                        let _ = tx.try_send(Command::CastEvent(id, event));
                    },
                ));
                self.cast_status = Default::default();
                self.cast_at = Instant::now();
                self.collections.clear();
                self.status = "Connecting to Cast receiver…".into();
                if !self.queue.items.is_empty() {
                    self.begin_play(Some(offset))?;
                }
            }
            Command::CastSource(id, g, media, offset, stream) => {
                if id == self.cast_id
                    && g == self.generation
                    && let Some(cast) = &self.cast
                {
                    cast.load_remote(g, media, offset, stream)?;
                }
            }
            Command::CastEvent(id, event) => {
                if id != self.cast_id || self.cast.is_none() {
                    return Ok(());
                }
                match event {
                    crate::casting::Event::Loaded(g, m, status) if g == self.generation => {
                        let m = *m;
                        self.listen_tick();
                        self.persist()?;
                        if self.cast_new_listen || self.listen.is_none() {
                            self.listen = Some(auvemir_core::scrobble::Listen {
                                media_id: m.id.clone(),
                                title: m.title.clone(),
                                artist: m.artist.clone(),
                                album: m.album.clone(),
                                duration_ms: m.duration_ms,
                                started_at: auvemir_core::scrobble::now(),
                                listened_ms: 0,
                            });
                            self.listen_queued = false;
                            self.display_lyrics(String::new())?;
                        }
                        self.current = Some(m.clone());
                        if self.cast_new_listen {
                            self.announce_now_playing();
                        }
                        self.cast_status = status;
                        self.cast_at = Instant::now();
                        self.last_tick = Instant::now();
                        self.db.played(&m.id)?;
                        self.last_save = Instant::now();
                        self.persist_queue()?;
                        self.activate_output()?;
                        self.status = format!("Casting {}", m.title);
                    }
                    crate::casting::Event::Status(g, status) if g == self.generation => {
                        self.listen_tick();
                        self.cast_status = status;
                        self.cast_at = Instant::now();
                    }
                    crate::casting::Event::Error(message) => {
                        let offset = self.position();
                        if let Some(cast) = self.cast.take() {
                            self.runtime.block_on(cast.close());
                        }
                        self.cast_id = self.cast_id.wrapping_add(1);
                        if let Some(m) = &self.current {
                            self.resume_from = Some((m.id.clone(), offset));
                        }
                        self.status = format!("Casting stopped: {message}");
                    }
                    _ => {}
                }
            }
            Command::Native(action, value, n) => match action.as_str() {
                "focus-pause" | "focus-play" | "service-stopped" => {
                    let previous = self.system_transport;
                    self.system_transport = true;
                    let result = if action == "focus-play" {
                        if let Some(targets) = self.transport.focus_gain() {
                            self.resume_audio(targets)
                        } else {
                            Ok(())
                        }
                    } else {
                        self.transport.interrupted = true;
                        self.handle(Command::MediaControl(
                            crate::controls::MediaControlEvent::Pause,
                        ))
                    };
                    self.system_transport = previous;
                    result?;
                    if action == "service-stopped" {
                        if self.recorder.as_ref().is_some_and(|r| !r.paused) {
                            self.action("record-pause")?;
                        }
                        if !self.ui.attached() {
                            let _ = self.tx.try_send(Command::Shutdown);
                        }
                    }
                }
                "import" => self.handle(Command::Import(value, n as i32))?,
                "notice" => self.status = value,
                "seek" => self.handle(Command::MediaControl(
                    crate::controls::MediaControlEvent::SetPosition(
                        crate::controls::MediaPosition(Duration::from_millis(n.max(0) as u64)),
                    ),
                ))?,
                "play-id" => {
                    let m = self
                        .db
                        .get(&value)?
                        .context("This item is no longer available")?;
                    if m.kind == MediaKind::Ambient {
                        self.transport.layers_paused = false;
                    }
                    self.queue.replace(vec![m], 0);
                    self.begin_play(None)?;
                }
                "play" => self.handle(Command::MediaControl(
                    crate::controls::MediaControlEvent::Play,
                ))?,
                "pause" => self.handle(Command::MediaControl(
                    crate::controls::MediaControlEvent::Pause,
                ))?,
                "record" => self.action("record")?,
                "record-pause" => self.action("record-pause")?,
                "resume-recording" => {
                    if self.recorder.as_ref().is_some_and(|r| r.paused) {
                        self.action("record-pause")?;
                    }
                }
                "stop-recording" => {
                    if self.recorder.is_some() {
                        self.action("record")?;
                    }
                }
                "pause-recording" => {
                    if self.recorder.as_ref().is_some_and(|r| !r.paused) {
                        self.action("record-pause")?;
                    }
                }
                "stop" => {
                    if self.recorder.is_some() {
                        self.action("record")?;
                    }
                    self.action("stop")?;
                }
                a if a.starts_with("file:") => self.files(&a[5..], &value)?,
                "toggle" => self.handle(Command::MediaControl(
                    crate::controls::MediaControlEvent::Toggle,
                ))?,
                "next" | "previous" | "noise-white" | "noise-pink" | "noise-brown"
                | "stop-ambient" | "ambient-toggle" => self.action(&action)?,
                _ => {}
            },
            Command::Shutdown => {}
            Command::ArtworkReady(url) => {
                self.artwork
                    .invalidate(&crate::artwork::cached_path(&self.base, &url).to_string_lossy());
                self.view_dirty = true;
            }
        }
        Ok(())
    }

    fn set_theme(&mut self, index: i32) -> Result<()> {
        self.theme_index = index.clamp(0, 7);
        self.db
            .set("appearance-theme", theme_id(self.theme_index))?;
        self.db
            .set("appearance-light", &(self.theme_index >= 4).to_string())?;
        Ok(())
    }
    fn action(&mut self, a: &str) -> Result<()> {
        if a.starts_with("server:")
            || a.starts_with("radio-")
            || a.starts_with("books-")
            || a.starts_with("podcasts-")
            || matches!(
                a,
                "subscriptions"
                    | "online-trending"
                    | "albums"
                    | "artists"
                    | "page-next"
                    | "page-prev"
            )
        {
            self.view_generation = self.view_generation.wrapping_add(1);
            self.loading = false;
        }
        if a == "cancel-import" {
            self.scan_cancel.store(true, Ordering::Relaxed);
            self.scan_generation = self.scan_generation.wrapping_add(1);
            self.scanning = false;
            self.status = format!(
                "Import canceled. {} completed files were saved.",
                self.scan_imported
            );
            return Ok(());
        }
        self.listen_tick();
        match a {
            "metadata-search" | "metadata-genius" | "metadata-recognize" => {
                let m = self.current.clone().context("Play a track first")?;
                let services = self.metadata_services()?;
                self.status = "Looking up track metadata…".into();
                let kind = a.to_owned();
                let key = if a == "metadata-genius" {
                    Some(crate::credentials::load("api-genius")?)
                } else if a == "metadata-recognize" {
                    Some(crate::credentials::load("api-acoustid")?)
                } else {
                    None
                };
                if a == "metadata-recognize" && m.is_remote() {
                    bail!("Audio recognition requires a local audio file");
                }
                self.job(async move {
                    let results = match kind.as_str() {
                        "metadata-genius" => {
                            services
                                .genius(key.as_deref().unwrap_or(""), &m.title, &m.artist)
                                .await?
                        }
                        "metadata-recognize" => {
                            let path = PathBuf::from(&m.location);
                            let (duration, fingerprint) = tokio::task::spawn_blocking(move || {
                                crate::fingerprint::calculate(&path)
                            })
                            .await??;
                            services
                                .recognize(key.as_deref().unwrap_or(""), duration, &fingerprint)
                                .await?
                        }
                        _ => services.search(&m.title, &m.artist).await?,
                    };
                    Ok(Command::Metadata(m, results))
                });
            }
            "metadata-cover" | "metadata-fanart" => {
                let m = self.current.clone().context("Play a track first")?;
                let candidate: auvemir_core::metadata::Candidate = serde_json::from_str(
                    &self
                        .db
                        .setting(&format!("metadata-refs:{}", m.id))?
                        .context("Choose a MusicBrainz metadata result first")?,
                )?;
                if a == "metadata-cover" {
                    let mut media = m;
                    media.artwork = Some(auvemir_core::metadata::Services::cover(
                        &candidate.release_id,
                    )?);
                    self.db.upsert(std::slice::from_ref(&media))?;
                    self.current = Some(media);
                    self.allow_remote_art = true;
                    self.refresh()?;
                    self.status = "Release artwork selected.".into();
                } else {
                    let services = self.metadata_services()?;
                    let key = crate::credentials::load("api-fanart")?;
                    self.job(async move {
                        let mut media = m;
                        media.artwork = Some(services.fanart(&key, &candidate.artist_id).await?);
                        Ok(Command::ArtSelected(media))
                    });
                }
            }
            "reference-playlists" => {
                self.collections = self
                    .db
                    .reference_summaries()?
                    .into_iter()
                    .map(|(id, name, matched, total)| {
                        (
                            format!("reference:{id}"),
                            name,
                            format!("{matched} of {total} matched"),
                        )
                    })
                    .collect();
                self.view.clear();
                self.view_dirty = true;
                self.page = 9;
                self.status = "Choose an imported playlist to view missing tracks.".into();
            }
            "edit-tags" => {
                let m = self.current.clone().context("Play a local track first")?;
                if m.is_remote() || m.provider != "local" {
                    bail!("Tag editing requires a local audio file");
                }
                self.job(async move {
                    let media = m.clone();
                    let tags = tokio::task::spawn_blocking(move || {
                        auvemir_core::library::read_tags(Path::new(&media.location))
                    })
                    .await??;
                    Ok(Command::TagDraft(m, tags))
                });
            }
            "discord-disconnect" => {
                #[cfg(not(any(target_os = "android", target_os = "ios")))]
                {
                    self.discord = None;
                    self.db.set("discord-app-id", "")?;
                    self.status = "Discord presence disabled.".into();
                }
            }
            "cast-discover" => {
                self.status = "Looking for Cast receivers…".into();
                self.job(async {
                    Ok(Command::CastDevices(
                        tokio::task::spawn_blocking(auvemir_core::cast::discover).await??,
                    ))
                });
            }
            "cast-disconnect" => {
                self.persist()?;
                let offset = self.position();
                if let Some(cast) = self.cast.take() {
                    self.runtime.block_on(cast.close());
                }
                self.cast_id = self.cast_id.wrapping_add(1);
                if let Some(m) = &self.current {
                    self.resume_from = Some((m.id.clone(), offset));
                }
                self.status = "Casting disconnected.".into();
            }
            "airplay" => {
                crate::platform::routes();
            }
            "sync-now" => {
                if self.sync_busy {
                    bail!("Sync is already running");
                }
                let config: auvemir_core::sync::WebDav = serde_json::from_str(
                    &self
                        .db
                        .setting("sync-config")?
                        .context("Configure encrypted sync first")?,
                )?;
                let secret: auvemir_core::sync::Secret =
                    serde_json::from_str(&crate::credentials::load("sync-webdav")?)?;
                if config.endpoint.trim().is_empty() {
                    bail!("Enter a WebDAV file URL in sync settings first");
                }
                let snapshot = self.db.sync_snapshot()?;
                self.sync_busy = true;
                self.status = "Synchronizing encrypted metadata…".into();
                self.job(async move {
                    Ok(Command::SyncFinished(
                        config
                            .exchange(&secret, snapshot)
                            .await
                            .map_err(|e| e.to_string()),
                    ))
                });
            }
            "sync-disconnect" => {
                crate::credentials::remove("sync-webdav")?;
                self.db.set("sync-config", "")?;
                self.status = "Sync disconnected. Local metadata is retained.".into();
            }
            "downloads" => {
                self.page = 9;
                self.view = self.db.downloaded_items()?;
                self.view_dirty = true;
                self.collections.clear();
                self.status = "Downloaded audio is ready for offline listening.".into();
            }
            "your-mix" => {
                self.view = self.db.media(Some(MediaKind::LocalMusic), "", 0, false)?;
                self.queue.replace(self.view.clone(), 0);
                self.queue.shuffle(rand::random());
                self.page = 8;
                self.refresh()?;
                self.begin_play(Some(0))?;
            }
            "record-pause" => {
                if let Some(r) = &mut self.recorder {
                    if r.paused {
                        crate::platform::activate_audio(true)?;
                    }
                    r.toggle_pause()?;
                    self.status = if r.paused {
                        "Recording paused."
                    } else {
                        "Recording resumed."
                    }
                    .into();
                }
            }
            "stop" => {
                self.persist()?;
                self.transport.stop();
                self.layer_generation = self.layer_generation.wrapping_add(1);
                self.generation += 1;
                if let Some(cast) = &self.cast {
                    cast.control("stop", 0.)?;
                    self.cast_status.playing = false;
                    self.cast_status.finished = false;
                }
                self.audio.stop_all();
                self.status = "Playback stopped.".into();
            }
            a if a.starts_with("server:") => {
                let id = a.trim_start_matches("server:");
                self.active_server = Some(id.into());
                self.db.set("active-server", id)?;
                self.query.clear();
                self.search_server_page(0, Some(id.into()), None)?;
            }
            a if a.starts_with("disconnect:") => {
                let id = a.trim_start_matches("disconnect:");
                if !self
                    .servers
                    .iter()
                    .any(|s| s.id == id && s.protocol == "invidious")
                {
                    crate::credentials::remove(id)?;
                }
                self.servers.retain(|s| s.id != id);
                self.services.retain(|s| s.id != id);
                if self.active_server.as_deref() == Some(id) {
                    self.active_server = self.servers.first().map(|s| s.id.clone());
                    self.db
                        .set("active-server", self.active_server.as_deref().unwrap_or(""))?;
                }
                self.db.remove_scrobbles(id)?;
                self.db
                    .set("servers", &serde_json::to_string(&self.servers)?)?;
                self.db
                    .set("scrobblers", &serde_json::to_string(&self.services)?)?;
                self.status = "Disconnected and removed the stored credential.".into();
            }
            "albums" | "artists" | "books-groups" => {
                let (kind, field) = if a == "books-groups" {
                    (MediaKind::Audiobook, "group_id")
                } else {
                    (
                        MediaKind::LocalMusic,
                        if a == "albums" { "album" } else { "artist" },
                    )
                };
                self.collections = self
                    .db
                    .groups(kind, field)?
                    .into_iter()
                    .map(|(value, title, n)| {
                        (
                            format!(
                                "group:{field}:{}",
                                url::form_urlencoded::byte_serialize(value.as_bytes())
                                    .collect::<String>()
                            ),
                            title,
                            format!("{n} items"),
                        )
                    })
                    .collect();
                self.view.clear();
                self.view_dirty = true;
                self.status = "Choose a collection.".into();
            }
            "analytics" => {
                self.collections = self
                    .db
                    .analytics()?
                    .into_iter()
                    .map(|(artist, plays, ms)| {
                        (
                            "".into(),
                            artist,
                            format!("{plays} plays · {} listened", format_time(ms)),
                        )
                    })
                    .collect();
                self.view.clear();
                self.view_dirty = true;
                self.status = format!(
                    "{} scrobbles waiting to sync.",
                    self.db.pending_scrobbles()?
                );
            }
            "eq-reset" => {
                for i in 0..10 {
                    self.audio.eq(i, 0.);
                }
                self.db
                    .set("equalizer", &serde_json::to_string(&[0.; 10])?)?;
            }
            "lyrics-earlier" => {
                self.lyric_offset = self.lyric_offset.saturating_sub(500);
                self.db
                    .set("lyrics-offset", &self.lyric_offset.to_string())?;
            }
            "lyrics-later" => {
                self.lyric_offset = self.lyric_offset.saturating_add(500);
                self.db
                    .set("lyrics-offset", &self.lyric_offset.to_string())?;
            }
            "ambient-save" => {
                let layers = self
                    .audio
                    .layer_ids()
                    .into_iter()
                    .map(|id| {
                        let gain = self.audio.layer_gain(&id);
                        (id, gain)
                    })
                    .collect::<Vec<_>>();
                self.db
                    .set("ambient-preset", &serde_json::to_string(&layers)?)?;
                self.status = "Your mix was saved.".into();
            }
            "ambient-restore" => {
                let layers: Vec<(String, f32)> = self
                    .db
                    .setting("ambient-preset")?
                    .and_then(|v| serde_json::from_str(&v).ok())
                    .context("Save a mix first")?;
                self.restore_mix(layers)?;
                self.status = "Saved mix restored.".into();
            }
            "subscriptions" => {
                self.collections = self
                    .db
                    .subscriptions()?
                    .into_iter()
                    .map(|(url, title, _)| (format!("feed:{url}"), title, url))
                    .collect();
                self.view.clear();
                self.view_dirty = true;
                self.status = "Choose a subscription to refresh it.".into();
            }
            "radio-top" => {
                let p = self.providers()?;
                self.status = "Loading popular radio stations…".into();
                self.collections.clear();
                self.view_job(async move { Ok(Command::Stations(p.radio("").await?)) });
            }
            "radio-countries" => {
                let p = self.providers()?;
                self.status = "Loading radio countries…".into();
                self.view_job(async move {
                    let countries = p.radio_countries().await?;
                    let cols = countries
                        .into_iter()
                        .map(|(c, count)| {
                            (format!("radio-country:{c}"), c, format!("{count} stations"))
                        })
                        .collect();
                    Ok(Command::Collections(cols))
                });
            }
            "radio-genres" => {
                let p = self.providers()?;
                self.status = "Loading radio genres…".into();
                self.view_job(async move {
                    let tags = p.radio_tags().await?;
                    let cols = tags
                        .into_iter()
                        .map(|(t, count)| {
                            (format!("radio-genre:{t}"), t, format!("{count} stations"))
                        })
                        .collect();
                    Ok(Command::Collections(cols))
                });
            }
            "online-trending" => {
                self.query.clear();
                self.search_server_page(0, None, None)?;
            }
            "books-local" => {
                self.collections.clear();
                self.query.clear();
                self.playlist_filter = None;
                self.page = 3;
                self.refresh()?;
                self.status = format!("{} local audiobooks in your library.", self.view.len());
            }
            "books-top" => {
                let p = self.providers()?;
                self.status = "Loading popular LibriVox audiobooks…".into();
                self.view_job(async move { Ok(Command::Catalog(p.books("").await?)) });
            }
            "books-genres" => {
                self.collections = auvemir_core::providers::book_genres()
                    .iter()
                    .map(|(id, name)| {
                        (
                            format!("book-genre:{id}"),
                            (*name).to_string(),
                            "LibriVox genre".to_string(),
                        )
                    })
                    .collect();
                self.view.clear();
                self.view_dirty = true;
                self.status = "Choose a genre to browse audiobooks.".into();
            }
            "podcasts-top" => {
                let p = self.providers()?;
                self.status = "Loading popular podcasts…".into();
                self.view_job(async move { Ok(Command::Catalog(p.podcasts("").await?)) });
            }
            "podcasts-genres" => {
                self.collections = auvemir_core::providers::podcast_genres()
                    .iter()
                    .map(|name| {
                        (
                            format!("podcast-genre:{name}"),
                            (*name).to_string(),
                            "Podcast genre".to_string(),
                        )
                    })
                    .collect();
                self.view.clear();
                self.view_dirty = true;
                self.status = "Choose a genre to browse podcasts.".into();
            }
            "pick-files" | "pick-folder" => {
                let page = self.page;
                let folder = a == "pick-folder";
                self.job(async move {
                    Ok(match crate::platform::pick_audio(page, folder).await? {
                        Some(path) => Command::Import(path, page),
                        None => Command::Notice("Choose audio using the file picker.".into()),
                    })
                });
            }
            a if a.starts_with("queue-add:") => {
                let id = a.trim_start_matches("queue-add:");
                let m = self.db.get(id)?.context("Item is no longer available")?;
                self.queue.items.push(m);
                self.persist_queue()?;
                self.status = "Added to the end of the queue.".into();
            }
            a if a.starts_with("queue-next:") => {
                let id = a.trim_start_matches("queue-next:");
                let m = self.db.get(id)?.context("Item is no longer available")?;
                self.queue.insert_next(m);
                self.persist_queue()?;
                self.status = "Added next in the queue.".into();
            }
            a if a.starts_with("queue-up:") || a.starts_with("queue-down:") => {
                let (direction, index) = a.split_once(':').context("Invalid queue action")?;
                let index: usize = index.parse()?;
                let target = if direction == "queue-up" {
                    index.saturating_sub(1)
                } else {
                    index.saturating_add(1)
                };
                if self.queue.move_item(index, target) {
                    self.persist_queue()?;
                    self.refresh()?;
                }
            }
            a if a.starts_with("queue-remove:") => {
                let i: usize = a.trim_start_matches("queue-remove:").parse()?;
                let active = i == self.queue.cursor;
                self.queue.remove(i);
                if active {
                    self.persist()?;
                    self.audio.stop();
                    if let Some(cast) = &self.cast {
                        cast.control("stop", 0.)?;
                        self.cast_status.playing = false;
                        self.cast_status.finished = false;
                    }
                    self.current = None;
                    self.generation += 1;
                }
                self.persist_queue()?;
                self.refresh()?;
            }
            "toggle" => {
                if let Some(cast) = &self.cast {
                    cast.control(if self.playing() { "pause" } else { "play" }, 0.)?;
                } else if self.audio.player.is_some() {
                    if !self.playing() {
                        self.activate_output()?;
                    }
                    self.audio.toggle();
                    self.persist()?;
                } else {
                    self.begin_play(None)?;
                }
            }
            "previous" => {
                if self.queue.previous() {
                    self.begin_play(Some(0))?;
                }
            }
            "next" => {
                if self.queue.advance(false) {
                    self.begin_play(Some(0))?;
                } else {
                    self.persist()?;
                    self.audio.stop();
                    if let Some(cast) = &self.cast {
                        cast.control("stop", 0.)?;
                        self.cast_status.playing = false;
                    }
                    self.status = "End of queue.".into();
                }
            }
            "shuffle" => {
                self.queue.shuffle(rand::random());
                self.persist_queue()?;
                if self.page == 8 {
                    self.refresh()?;
                }
                self.status = "Upcoming tracks shuffled.".into();
            }
            "repeat" => self.queue.cycle_repeat(),
            "clear-queue" => {
                self.persist()?;
                self.generation += 1;
                self.audio.stop();
                if let Some(cast) = &self.cast {
                    cast.control("stop", 0.)?;
                    self.cast_status.playing = false;
                    self.cast_status.finished = false;
                }
                self.current = None;
                self.listen = None;
                self.queue = Queue::default();
                self.persist_queue()?;
                self.refresh()?;
            }
            "page-next" => {
                if self.page == 2 {
                    if self.remote_has_more {
                        self.search_server_page(
                            self.offset.saturating_add(100),
                            self.remote_server_id.clone(),
                            self.remote_path.clone(),
                        )?;
                    }
                } else {
                    self.offset += 100;
                    self.refresh()?;
                }
            }
            "page-prev" => {
                if self.page == 2 {
                    if self.offset > 0 {
                        self.search_server_page(
                            self.offset.saturating_sub(100),
                            self.remote_server_id.clone(),
                            self.remote_path.clone(),
                        )?;
                    }
                } else {
                    self.offset = self.offset.saturating_sub(100);
                    self.refresh()?;
                }
            }
            "back15" | "forward30" => {
                let pos = self.position();
                let pos = if a == "back15" {
                    pos.saturating_sub(15_000)
                } else {
                    pos.saturating_add(30_000).min(self.duration())
                };
                self.seek_to(pos)?;
            }
            "favorite-current" => {
                if let Some(m) = self.current.clone() {
                    self.handle(Command::Favorite(m.id))?;
                }
            }
            "lyrics" => {
                let m = self.current.clone().context("Play a track first")?;
                if let Some(text) = self.db.lyrics(&m.id)? {
                    self.display_lyrics(text)?;
                } else if !m.is_remote()
                    && let Ok(Some(text)) =
                        auvemir_core::library::embedded_lyrics(Path::new(&m.location))
                {
                    self.handle(Command::Lyrics(m.id, text))?;
                } else if let Ok(text) =
                    std::fs::read_to_string(Path::new(&m.location).with_extension("lrc"))
                {
                    self.handle(Command::Lyrics(m.id, text))?;
                } else {
                    if self.lyric_requests.contains_key(&m.id) {
                        return Ok(());
                    }
                    anyhow::ensure!(
                        self.lyric_requests.len() < 4,
                        "Lyrics lookups are busy. Try again shortly"
                    );
                    let p = self.providers()?;
                    self.lyric_request_counter = self.lyric_request_counter.wrapping_add(1);
                    let request = self.lyric_request_counter;
                    self.lyric_requests.insert(m.id.clone(), request);
                    self.status = "Looking up lyrics…".into();
                    self.job(async move {
                        let result = p.lyrics(&m).await.map_err(|e| e.to_string());
                        Ok(Command::LyricsFetched(m.id, request, result))
                    });
                }
            }
            "book-chapters" => {
                let m = self
                    .current
                    .clone()
                    .context("Play a local M4B audiobook first")?;
                anyhow::ensure!(
                    m.kind == MediaKind::Audiobook
                        && !m.is_remote()
                        && Path::new(&m.location)
                            .extension()
                            .and_then(|extension| extension.to_str())
                            .is_some_and(|extension| {
                                extension.eq_ignore_ascii_case("m4b")
                                    || extension.eq_ignore_ascii_case("m4a")
                            }),
                    "Embedded chapters are available for local M4B or M4A audiobooks"
                );
                self.chapter_request_generation = self.chapter_request_generation.wrapping_add(1);
                let request = self.chapter_request_generation;
                let id = m.id;
                let path = PathBuf::from(m.location);
                self.chapter_media_id = None;
                self.chapter_positions.clear();
                self.collections.clear();
                self.status = "Reading embedded audiobook chapters…".into();
                self.job(async move {
                    let result = tokio::task::spawn_blocking(move || {
                        auvemir_core::library::embedded_chapters(&path)
                    })
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|chapters| chapters.map_err(|error| error.to_string()));
                    Ok(Command::EmbeddedChapters(id, request, result))
                });
            }
            "bookmarks" => {
                let m = self
                    .current
                    .as_ref()
                    .context("Play a local audiobook first")?;
                let bookmarks = self.db.bookmarks(&m.id)?;
                self.collections = bookmarks
                    .iter()
                    .map(|b| {
                        (
                            format!("bookmark:{}", b.id),
                            b.name.clone(),
                            format_time(b.position_ms),
                        )
                    })
                    .collect();
                self.bookmark_ids = bookmarks
                    .iter()
                    .map(|b| (format!("bookmark:{}", b.id), b.position_ms))
                    .collect();
                self.status = format!("{} bookmarks. Choose one to resume.", bookmarks.len());
            }
            "download" => {
                if let Some(cancel) = self.download_cancel.take() {
                    let _ = cancel.send(true);
                    self.status = "Download paused; choose Download to resume".into();
                } else {
                    let m = self
                        .current
                        .clone()
                        .context("Play a public-domain book or podcast first")?;
                    let providers = self.providers()?;
                    let server = self
                        .servers
                        .iter()
                        .find(|s| m.provider == format!("server:{}", s.id))
                        .cloned();
                    let secret = server
                        .as_ref()
                        .map(|s| crate::credentials::load(&s.id))
                        .transpose()?;
                    let default_client = providers.client;
                    let dir = self.base.join("downloads");
                    let (tx, rx) = tokio::sync::watch::channel(false);
                    self.download_cancel = Some(tx);
                    let notice = self.tx.clone();
                    self.status = "Downloading authorized audio…".into();
                    self.job(async move {
                        let result = async {
                            let (source, downloader) =
                                if let (Some(server), Some(secret)) = (server, secret) {
                                    let resolved = server.resolve(&secret, &m).await?;
                                    let mut source = m.clone();
                                    source.location = resolved.location;
                                    (source, Downloader::personal_server(resolved.client))
                                } else {
                                    (m.clone(), Downloader::new(default_client))
                                };
                            downloader
                                .download(&source, &dir, rx, move |p| {
                                    let _ = notice.try_send(Command::Notice(format!(
                                        "Downloading {:.1} MiB{}",
                                        p.received as f64 / 1048576.0,
                                        p.total
                                            .map(|n| format!(" / {:.1} MiB", n as f64 / 1048576.0))
                                            .unwrap_or_default()
                                    )));
                                })
                                .await
                        }
                        .await;
                        Ok(match result {
                            Ok(path) => Command::Downloaded(m.id, path),
                            Err(e) => Command::DownloadFailed(e.to_string()),
                        })
                    });
                }
            }
            "record" => {
                if let Some(r) = self.recorder.take() {
                    let (path, dropped) = r.stop()?;
                    let m = auvemir_core::library::read_media(&path, MediaKind::Recording)?;
                    self.db.upsert(&[m])?;
                    self.status = format!("Recording saved. {dropped} samples were dropped.");
                    self.refresh()?;
                } else {
                    if !crate::platform::microphone_ready()? {
                        self.status = "Allow microphone access to record.".into();
                        return Ok(());
                    }
                    crate::platform::activate_audio(true)?;
                    self.recording_generation = self.recording_generation.wrapping_add(1);
                    let generation = self.recording_generation;
                    let tx = self.tx.clone();
                    self.recorder =
                        Some(Recorder::start(&self.base.join("recordings"), move || {
                            let _ = tx.try_send(Command::CaptureFinished(generation));
                        })?);
                    self.status = "Recording microphone. Choose Stop recording to save.".into();
                }
            }
            "noise-white" | "noise-pink" | "noise-brown" => {
                self.transport.layers_paused = false;
                let kind = a.strip_prefix("noise-").unwrap();
                if !self.audio.has_layer(kind) {
                    self.activate_output()?;
                }
                let active = self.audio.noise(kind)?;
                self.status = format!("{kind} noise {}", if active { "on" } else { "off" });
            }
            "stop-ambient" => {
                self.audio.stop_layers();
                self.layer_generation = self.layer_generation.wrapping_add(1);
                self.transport.pause_layers();
                self.status = "Ambient mix stopped.".into();
            }
            "ambient-toggle" => {
                if self.audio.layers_playing() {
                    self.audio.pause_layers();
                    self.transport.pause_layers();
                    self.status = "Ambient mix paused.".into();
                } else if self.audio.has_layers() {
                    self.transport.layers_paused = false;
                    self.activate_output()?;
                    self.audio.resume_layers();
                    self.status = "Ambient mix resumed.".into();
                }
            }
            "refresh-feeds" => {
                let p = self.providers()?;
                for (url, _, kind) in self.db.subscriptions()? {
                    let p = p.clone();
                    self.view_job(async move {
                        let (title, items) = p.feed(&url, kind).await?;
                        Ok(Command::Feed(url, title, kind, items))
                    });
                }
                self.status = "Refreshing subscriptions…".into();
            }
            _ => bail!("Unknown command"),
        }
        Ok(())
    }
    fn begin_play(&mut self, explicit_offset: Option<u64>) -> Result<()> {
        let mut m = self
            .queue
            .current()
            .cloned()
            .context("Choose a track from your library first")?;
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.id != m.id)
        {
            self.chapter_request_generation = self.chapter_request_generation.wrapping_add(1);
            self.chapter_media_id = None;
            self.chapter_positions.clear();
            self.collections
                .retain(|(id, _, _)| !id.starts_with("chapter:"));
        }
        if let Some(path) = self.db.download_path(&m.id)?
            && Path::new(&path).is_file()
        {
            m.location = path;
        }
        if self.rooms.hosting() && !m.seekable() {
            self.leave_room();
        }
        let explicit_offset = explicit_offset.or_else(|| {
            self.resume_from
                .take()
                .filter(|(id, _)| id == &m.id)
                .map(|(_, position)| position)
        });
        let offset = if let Some(offset) = explicit_offset {
            offset
        } else if m.kind.long_form() && !m.is_remote() {
            let saved = self.db.progress(&m.id)?;
            if m.duration_ms > 0 && saved.saturating_add(1000) >= m.duration_ms {
                0
            } else {
                saved
            }
        } else {
            0
        };
        let offset = if m.duration_ms > 0 {
            offset.min(m.duration_ms.saturating_sub(1000))
        } else {
            offset
        };
        if m.kind == MediaKind::Ambient {
            let id = m.id.clone();
            let generation = self.layer_generation;
            self.job(async move {
                Ok(Command::Layer(
                    generation,
                    id,
                    audio::prepare(m, 0, true).await?,
                ))
            });
            return Ok(());
        }
        self.generation += 1;
        let g = self.generation;
        let server = self
            .servers
            .iter()
            .find(|s| {
                m.location.starts_with("auvemir://server/")
                    && m.provider == format!("server:{}", s.id)
            })
            .cloned();
        let secret = server
            .as_ref()
            .map(|s| {
                if s.protocol == "invidious" {
                    Ok(String::new())
                } else {
                    crate::credentials::load(&s.id)
                }
            })
            .transpose()?;
        if let Some(cast) = &self.cast {
            self.cast_new_listen =
                self.current.as_ref().is_none_or(|old| old.id != m.id) || self.cast_status.finished;
            if let (Some(server), Some(secret)) = (server, secret) {
                let id = self.cast_id;
                self.job(async move {
                    let stream = server.resolve(&secret, &m).await?;
                    Ok(Command::CastSource(id, g, m, offset, stream))
                });
            } else if m.provider == "custom-radio" && m.is_remote() {
                let stream = auvemir_core::servers::Stream {
                    location: m.location.clone(),
                    client: auvemir_core::network::client(true)?,
                };
                cast.load_remote(g, m, offset, stream)?;
            } else {
                cast.load(g, m, offset)?;
            }
            self.cast_status.position_ms = offset;
            self.cast_at = Instant::now();
            self.cast_status.playing = false;
            self.cast_status.finished = false;
            self.status = "Loading audio on Cast receiver…".into();
            return Ok(());
        }
        self.status = format!("Opening {}…", m.title);
        let dsp = self.audio.dsp.clone();
        self.job(async move {
            let result: Result<Prepared> = async {
                let p = if let (Some(server), Some(secret)) = (server, secret) {
                    let resolved = server.resolve(&secret, &m).await?;
                    let mut source = m.clone();
                    source.location = resolved.location;
                    audio::prepare_config(source, 0, false, Some(resolved.client), dsp)
                        .await
                        .map_err(|_| {
                            anyhow::anyhow!("Could not decode the server's audio stream")
                        })?
                } else {
                    audio::prepare_config(m.clone(), offset, false, None, dsp).await?
                };
                Ok(p)
            }
            .await;
            Ok(match result {
                Ok(p) => Command::Prepared(g, m, offset, p),
                Err(e) => Command::PrepareFailed(g, e.to_string()),
            })
        });
        Ok(())
    }
    fn persist_queue(&self) -> Result<()> {
        self.db.save_queue(
            &self
                .queue
                .items
                .iter()
                .map(|m| m.id.clone())
                .collect::<Vec<_>>(),
            self.queue.cursor,
        )
    }
    fn persist(&mut self) -> Result<()> {
        if let Some(listen) = &self.listen
            && listen.listened_ms > 0
        {
            self.db.record_listen(listen, &[])?;
        }
        if let Some(m) = &self.current
            && m.kind.long_form()
        {
            self.db.save_progress(&m.id, self.position())?;
        }
        self.sync_remote_progress()?;
        self.persist_queue()
    }
    fn audiobook_server_progress(&self) -> Option<(String, String, u64, u64)> {
        let media = self.current.as_ref()?;
        if media.kind != MediaKind::Audiobook || media.group_id.is_empty() {
            return None;
        }
        let server_id = media.provider.strip_prefix("server:")?;
        if !self
            .servers
            .iter()
            .any(|server| server.id == server_id && server.protocol == "audiobookshelf")
        {
            return None;
        }
        let current_duration = self.duration().max(media.duration_ms);
        let mut position_before = 0u64;
        let mut total_duration = 0u64;
        for track in self.queue.items.iter().filter(|track| {
            track.kind == MediaKind::Audiobook
                && track.provider == media.provider
                && track.group_id == media.group_id
        }) {
            let duration = if track.id == media.id {
                current_duration
            } else {
                track.duration_ms
            };
            total_duration = total_duration.saturating_add(duration);
            if track.track_number < media.track_number {
                position_before = position_before.saturating_add(duration);
            }
        }
        if total_duration == 0 {
            total_duration = current_duration;
        }
        if total_duration == 0 {
            return None;
        }
        let position = position_before
            .saturating_add(self.position())
            .min(total_duration);
        Some((
            server_id.to_owned(),
            media.group_id.clone(),
            position,
            total_duration,
        ))
    }
    fn sync_remote_progress(&mut self) -> Result<()> {
        if self.remote_progress_busy {
            return Ok(());
        }
        let Some((server_id, item_id, position, duration)) = self.audiobook_server_progress()
        else {
            return Ok(());
        };
        let snapshot = (item_id.clone(), position);
        let changed_while_paused = !self.playing()
            && self
                .remote_progress_snapshot
                .as_ref()
                .is_none_or(|last| last.0 != snapshot.0 || last.1.abs_diff(snapshot.1) >= 2_000);
        if self.last_remote_progress_sync.elapsed() < Duration::from_secs(30)
            && !changed_while_paused
        {
            return Ok(());
        }
        let Some(server) = self
            .servers
            .iter()
            .find(|server| server.id == server_id)
            .cloned()
        else {
            return Ok(());
        };
        let secret = match crate::credentials::load(&server.id) {
            Ok(secret) => secret,
            Err(error) => {
                self.last_remote_progress_sync = Instant::now();
                self.status = format!("Audiobookshelf progress sync unavailable: {error}");
                return Ok(());
            }
        };
        let is_finished = duration > 0 && position.saturating_add(1_000) >= duration;
        self.remote_progress_busy = true;
        self.last_remote_progress_sync = Instant::now();
        self.remote_progress_snapshot = Some(snapshot);
        self.job(async move {
            let result = server
                .update_book_progress(&secret, &item_id, position, duration, is_finished)
                .await
                .map_err(|error| error.to_string());
            Ok(Command::RemoteProgressSynced(item_id, result))
        });
        Ok(())
    }
    fn tick(&mut self) -> Result<()> {
        self.listen_tick();
        if self
            .recorder
            .as_ref()
            .is_some_and(Recorder::writer_finished)
        {
            if let Err(error) = self.action("record") {
                self.status = error.to_string();
            }
            self.render()?;
        }
        if let Some(event) = self.rooms.take_event() {
            match event {
                auvemir_core::listen::Event::State(state, offset) => {
                    self.rooms.latest = Some((state, offset))
                }
                auvemir_core::listen::Event::Disconnected(error) => {
                    self.audio.pause();
                    self.persist()?;
                    self.leave_room();
                    self.status = error;
                }
            }
            self.render()?;
        }
        if self.rooms.following
            && let Err(error) = self.apply_room()
        {
            self.audio.pause();
            self.leave_room();
            self.status = error.to_string();
            self.render()?;
        }
        if let Err(error) = self.rooms.publish(
            self.current.as_ref(),
            self.position(),
            self.duration(),
            self.playing(),
            self.audio.speed,
        ) {
            self.leave_room();
            self.status = error.to_string();
            self.render()?;
        }
        if let Some(listen) = &self.listen
            && !self.listen_queued
            && listen.eligible()
            && self
                .current
                .as_ref()
                .is_some_and(|m| matches!(m.kind, MediaKind::LocalMusic | MediaKind::OnlineMusic))
        {
            let ids = self
                .services
                .iter()
                .filter(|s| s.enabled)
                .map(|s| s.id.clone())
                .collect::<Vec<_>>();
            self.db.record_listen(listen, &ids)?;
            self.listen_queued = true;
        }
        if self.services.iter().any(|s| s.enabled)
            && !self.scrobble_busy
            && Instant::now() >= self.next_scrobble
        {
            let mut pending = Vec::new();
            for (id, service_id, listen) in self.db.scrobbles(auvemir_core::scrobble::now())? {
                if let Some(service) = self
                    .services
                    .iter()
                    .find(|s| s.id == service_id && s.enabled)
                    && let Ok(secret) = crate::credentials::load(&service.id)
                {
                    pending.push((id, service.clone(), secret, listen));
                }
            }
            self.next_scrobble = Instant::now() + Duration::from_secs(30);
            if !pending.is_empty() {
                self.scrobble_busy = true;
                self.job(async move {
                    let mut results = Vec::new();
                    for (id, service, secret, listen) in pending {
                        let success = auvemir_core::scrobble::submit(&service, &secret, &listen)
                            .await
                            .is_ok();
                        results.push((id, success));
                    }
                    Ok(Command::Scrobbled(results))
                });
            }
        }
        if self.sleep.is_some_and(|s| Instant::now() >= s) {
            self.leave_room();
            self.persist()?;
            self.audio.stop_all();
            self.transport.stop();
            self.layer_generation = self.layer_generation.wrapping_add(1);
            if let Some(cast) = &self.cast {
                cast.control("stop", 0.)?;
                self.cast_status.playing = false;
                self.cast_status.finished = false;
            }
            self.generation += 1;
            self.sleep = None;
            self.status = "Sleep timer finished.".into();
            self.render()?;
        }
        if self
            .cast
            .as_ref()
            .map(|_| self.cast_status.finished)
            .unwrap_or_else(|| self.audio.ended())
        {
            self.persist()?;
            self.cast_status.finished = false;
            self.listen = None;
            if !self.rooms.following && self.queue.advance(true) {
                self.audio.stop();
                self.begin_play(Some(0))?;
            } else {
                self.audio.stop();
                self.cast_status.playing = false;
                self.status = "End of queue.".into();
            }
            self.render()?;
        }
        if self.playing() {
            if self.last_save.elapsed() >= Duration::from_secs(10) {
                self.persist()?;
                self.last_save = Instant::now();
            }
            self.render_player()?;
        } else if self.recorder.is_some() {
            self.render_player()?;
        }
        Ok(())
    }
    fn refresh(&mut self) -> Result<()> {
        self.view_dirty = true;
        self.view = if self.page == 8 {
            self.queue.items.clone()
        } else if let Some(id) = self.playlist_filter {
            self.db.playlist_items(id)?
        } else if self.page == 0 {
            let recent = self.db.recent()?;
            self.home_recent = !recent.is_empty();
            if self.home_recent {
                recent
            } else {
                self.db
                    .media_sorted(Some(MediaKind::LocalMusic), "", 0, false, self.sort)?
            }
        } else if self.page == 11 {
            vec![]
        } else {
            self.db.media_sorted(
                kind_for_page(self.page),
                &self.query,
                self.offset,
                self.page == 10,
                self.sort,
            )?
        };
        Ok(())
    }
    fn listen_tick(&mut self) {
        let elapsed = self.last_tick.elapsed();
        self.last_tick = Instant::now();
        if self.playing()
            && let Some(listen) = &mut self.listen
        {
            listen.listened_ms = listen
                .listened_ms
                .saturating_add(elapsed.as_millis().min(1500) as u64);
        }
    }
    fn render(&mut self) -> Result<()> {
        if self.current.is_none() && !self.lyrics_text.is_empty() {
            self.display_lyrics(String::new())?;
        }
        crate::platform::recording(
            self.recorder.is_some(),
            self.recorder.as_ref().is_some_and(|r| r.paused),
        );
        if !self.ui.attached() {
            return self.render_player();
        }
        if self.allow_remote_art {
            let urls = self
                .view
                .iter()
                .take(32)
                .chain(self.current.iter())
                .filter_map(|m| m.artwork.as_ref())
                .filter(|u| u.starts_with("http"))
                .cloned()
                .collect::<Vec<_>>();
            for url in urls {
                if crate::artwork::cached_path(&self.base, &url).is_file()
                    || self.art_attempts.contains(&url)
                {
                    continue;
                }
                if self.art_attempts.len() >= 512 {
                    self.art_attempts.clear();
                }
                self.art_attempts.insert(url.clone());
                let base = self.base.clone();
                let slots = self.art_slots.clone();
                self.job(async move {
                    let _ = crate::artwork::fetch(base, url.clone(), slots).await;
                    Ok(Command::ArtworkReady(url))
                });
            }
        }

        let tracks = if self.view_dirty {
            let ids = self.view.iter().map(|m| m.id.as_str()).collect::<Vec<_>>();
            let downloaded = self.db.download_flags(&ids).unwrap_or_else(|_| {
                ids.into_iter()
                    .filter(|id| self.db.download_path(id).ok().flatten().is_some())
                    .map(str::to_owned)
                    .collect()
            });
            Some(
                self.view
                    .iter()
                    .map(|m| {
                        (m.clone(), downloaded.contains(&m.id), {
                            let path =
                                m.artwork
                                    .as_ref()
                                    .filter(|u| u.starts_with("http"))
                                    .map(|u| {
                                        crate::artwork::cached_path(&self.base, u)
                                            .to_string_lossy()
                                            .into_owned()
                                    });
                            self.artwork.get(path.as_deref().or(m.artwork.as_deref()))
                        })
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        };
        self.view_dirty = false;
        let art_url = self.current.as_ref().and_then(|m| m.artwork.as_deref());
        let art_path = art_url.filter(|u| u.starts_with("http")).map(|u| {
            crate::artwork::cached_path(&self.base, u)
                .to_string_lossy()
                .into_owned()
        });
        let current_art = self.artwork.get(art_path.as_deref().or(art_url));
        let collections: Vec<NamedRow> = self
            .collections
            .iter()
            .map(|(id, title, subtitle)| NamedRow {
                id: id.clone().into(),
                title: title.clone().into(),
                subtitle: subtitle.clone().into(),
            })
            .collect();
        let playlists: Vec<NamedRow> = self
            .db
            .playlists()?
            .into_iter()
            .map(|(id, title)| NamedRow {
                id: id.to_string().into(),
                title: title.into(),
                subtitle: "".into(),
            })
            .collect();
        let (heading, subheading) = headings(self.page);
        let connections = self
            .servers
            .iter()
            .map(|s| NamedRow {
                id: s.id.clone().into(),
                title: s.name.clone().into(),
                subtitle: s.protocol.clone().into(),
            })
            .chain(self.services.iter().map(|s| NamedRow {
                id: s.id.clone().into(),
                title: s.name.clone().into(),
                subtitle: s.protocol.clone().into(),
            }))
            .collect::<Vec<_>>();
        let eq = self
            .audio
            .dsp
            .gains()
            .iter()
            .enumerate()
            .map(|(i, g)| EqBand {
                name: format!("{} Hz", crate::dsp::FREQUENCIES[i] as i32).into(),
                gain: *g,
            })
            .collect::<Vec<_>>();
        let layers = self
            .audio
            .layer_ids()
            .into_iter()
            .map(|id| {
                let gain = self.audio.layer_gain(&id) * 100.;
                let title = self
                    .db
                    .get(&id)
                    .ok()
                    .flatten()
                    .map(|m| m.title)
                    .unwrap_or_else(|| format!("{} noise", id));
                LayerRow {
                    id: id.into(),
                    name: title.into(),
                    volume: gain,
                }
            })
            .collect::<Vec<_>>();
        let presets = self
            .db
            .ambient_presets()
            .unwrap_or_default()
            .into_iter()
            .map(|p| NamedRow {
                id: p.name.clone().into(),
                title: p.name.into(),
                subtitle: format!("{} layers", p.layers.len()).into(),
            })
            .collect::<Vec<_>>();
        let sort = self.sort.index();
        let theme_index = self.theme_index;
        let reduced_motion = self.reduce_motion || crate::motion::system_reduced();
        let force_reduce = self.reduce_motion;
        let loading = self.loading;
        let scanning = self.scanning;
        let home_recent = self.home_recent;
        let server_names = self
            .servers
            .iter()
            .map(|s| slint::SharedString::from(s.name.as_str()))
            .collect::<Vec<_>>();
        let active_index = self
            .servers
            .iter()
            .position(|s| Some(&s.id) == self.active_server.as_ref())
            .unwrap_or(0) as i32;
        let status = self.status.clone();
        let count = self.db.count()? as i32;
        let page = self.page;
        let offset = self.offset as i32;
        let paginated = self.playlist_filter.is_none() && !matches!(page, 0 | 8 | 11);
        let pagination_visible =
            paginated && (self.view.len() == 100 || offset > 0 || self.remote_has_more);
        let next_page_enabled = paginated
            && if page == 2 {
                self.remote_has_more
            } else {
                self.view.len() == 100
            };
        let record = self.recorder.is_some();
        let recording_paused = self.recorder.as_ref().is_some_and(|r| r.paused);
        crate::platform::recording(record, recording_paused);
        let downloading = self.download_cancel.is_some();
        let volume = self.audio.volume * 100.0;
        let speed = self.audio.speed;
        let casting = self.cast.is_some();
        let invitation = self.rooms.invitation.clone();
        let room_status = if self.rooms.hosting() {
            "Hosting"
        } else if self.rooms.following {
            "Following"
        } else if self.rooms.cancel.is_some() {
            "Joining"
        } else {
            "Off"
        };
        self.ui.upgrade_in_event_loop(move |u| {
            u.set_theme_index(theme_index);
            u.set_reduced_motion(reduced_motion);
            u.set_force_reduced_motion(force_reduce);
            u.set_sort_index(sort);
            u.set_loading(loading);
            u.set_scanning(scanning);
            u.set_home_recent(home_recent);
            u.set_server_names(ModelRc::new(VecModel::from(server_names)));
            u.set_active_server_index(active_index);
            u.set_room_invitation(invitation.into());
            u.set_room_status(room_status.into());
            u.set_casting(casting);
            u.set_page(page);
            u.set_page_offset(offset);
            u.set_next_page_enabled(next_page_enabled);
            u.set_pagination_visible(pagination_visible);
            u.set_heading(heading.into());
            u.set_subheading(subheading.into());
            u.set_status(status.into());
            if let Some(tracks) = tracks {
                let tracks = tracks
                    .into_iter()
                    .map(|(m, downloaded, artwork)| TrackRow {
                        id: m.id.into(),
                        title: m.title.into(),
                        artist: if m.artist.is_empty() {
                            m.kind.as_str().replace('_', " ").into()
                        } else {
                            m.artist.into()
                        },
                        album: m.album.into(),
                        duration: if m.kind == MediaKind::Radio {
                            "LIVE".into()
                        } else {
                            format_time(m.duration_ms).into()
                        },
                        favorite: m.favorite,
                        kind: m.kind.as_str().into(),
                        downloaded,
                        artwork: artwork.image(),
                    })
                    .collect::<Vec<_>>();
                u.set_tracks(ModelRc::new(VecModel::from(tracks)));
            }
            u.set_now_artwork(current_art.image());
            u.set_collections(ModelRc::new(VecModel::from(collections)));
            u.set_saved_playlists(ModelRc::new(VecModel::from(playlists)));
            u.set_connections(ModelRc::new(VecModel::from(connections)));
            u.set_eq_bands(ModelRc::new(VecModel::from(eq)));
            u.set_ambient_layers(ModelRc::new(VecModel::from(layers)));
            u.set_ambient_presets(ModelRc::new(VecModel::from(presets)));
            u.set_library_count(count);
            u.set_recording(record);
            u.set_recording_paused(recording_paused);
            u.set_downloading(downloading);
            u.set_volume(volume);
            u.set_speed(speed);
        })?;
        self.render_player()
    }
    fn render_player(&self) -> Result<()> {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        if let Some(discord) = &self.discord {
            let activity = self
                .current
                .as_ref()
                .filter(|m| {
                    self.playing()
                        && matches!(m.kind, MediaKind::LocalMusic | MediaKind::OnlineMusic)
                })
                .map(|m| {
                    let now = auvemir_core::scrobble::now();
                    let speed = if self.cast.is_some() {
                        1.0
                    } else {
                        self.audio.speed as f64
                    };
                    let started = now - (self.position() as f64 / speed / 1000.) as i64;
                    auvemir_core::discord::Activity {
                        title: m.title.clone(),
                        artist: m.artist.clone(),
                        album: m.album.clone(),
                        started,
                        ends: (self.duration() > 0)
                            .then_some(started + (self.duration() as f64 / speed / 1000.) as i64),
                    }
                });
            discord.update(activity);
        }

        let (title, artist, favorite, can_seek, can_download) = self
            .current
            .as_ref()
            .map(|m| {
                (
                    m.title.clone(),
                    m.artist.clone(),
                    m.favorite,
                    if self.cast.is_some() {
                        m.kind != MediaKind::Radio && m.duration_ms > 0
                    } else {
                        m.seekable()
                    },
                    m.is_remote() && matches!(m.kind, MediaKind::Audiobook | MediaKind::Podcast),
                )
            })
            .unwrap_or_else(|| {
                (
                    "Nothing playing".into(),
                    "Choose a track from your library".into(),
                    false,
                    false,
                    false,
                )
            });
        let has_current = self.current.is_some();
        let current_id = self
            .current
            .as_ref()
            .map(|m| m.id.clone())
            .unwrap_or_default();
        let has_embedded_chapters = self.current.as_ref().is_some_and(|m| {
            m.kind == MediaKind::Audiobook
                && !m.is_remote()
                && Path::new(&m.location)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        extension.eq_ignore_ascii_case("m4b")
                            || extension.eq_ignore_ascii_case("m4a")
                    })
        });
        let position = self.position();
        let duration = self.duration();
        let playing = self.playing();
        let ratio = if duration == 0 {
            0.0
        } else {
            (position as f32 / duration as f32).clamp(0.0, 1.0)
        };
        let repeat = format!(
            "Repeat {}",
            match self.queue.repeat {
                auvemir_core::queue::Repeat::Off => "off",
                auvemir_core::queue::Repeat::All => "all",
                auvemir_core::queue::Repeat::One => "one",
            }
        );
        let mut media = self.current.clone();
        if let Some(media) = &mut media {
            media.duration_ms = duration;
        }
        let ambient_active = self.audio.layers_playing();
        let ambient_controls = !playing && self.audio.has_layers();
        if ambient_controls {
            media = Some(Media::new(
                MediaKind::Ambient,
                "ambient",
                "auvemir:ambient",
                "Ambient mix",
            ));
        }
        let controls_playing = playing || ambient_active;
        let controls_position = if ambient_controls { 0 } else { position };
        let controls_seekable = !ambient_controls && can_seek && duration > 0;
        let speed = if self.cast.is_some() {
            1.
        } else {
            self.audio.speed
        };
        let volume = self.audio.volume;
        let lyric_time = position.saturating_add_signed(self.lyric_offset);
        let active_lyric_index = auvemir_core::lyrics::active_line(&self.lyric_lines, lyric_time);
        let active_lyric = active_lyric_index
            .map(|i| self.lyric_lines[i].text.clone())
            .unwrap_or_default();
        let recording_level = self
            .recorder
            .as_ref()
            .filter(|r| !r.paused)
            .map(|r| f32::from_bits(r.level.load(Ordering::Relaxed)))
            .unwrap_or(0.);
        #[cfg(any(target_os = "android", target_os = "ios"))]
        crate::controls::update(
            media.as_ref(),
            controls_playing,
            controls_position,
            volume,
            if ambient_controls { 1.0 } else { speed },
            controls_seekable,
        );
        self.ui.upgrade_in_event_loop(move |u| {
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            crate::controls::update(
                media.as_ref(),
                controls_playing,
                controls_position,
                volume,
                if ambient_controls { 1.0 } else { speed },
                controls_seekable,
            );
            u.set_ambient_playing(ambient_active);
            u.set_active_lyric(active_lyric.into());
            u.set_active_lyric_index(active_lyric_index.map(|i| i as i32).unwrap_or(-1));
            u.set_recording_level(recording_level);
            u.set_current_id(current_id.into());
            u.set_has_embedded_chapters(has_embedded_chapters);
            u.set_now_title(title.into());
            u.set_now_artist(artist.into());
            u.set_current_favorite(favorite);
            u.set_can_seek(can_seek && duration > 0);
            u.set_can_download(can_download);
            u.set_playing(playing);
            u.set_elapsed(format_time(position).into());
            u.set_total(if !has_current {
                "0:00".into()
            } else if duration == 0 {
                "LIVE".into()
            } else {
                format_time(duration).into()
            });
            u.set_progress(ratio);
            u.set_repeat_label(repeat.into());
        })?;
        Ok(())
    }
    fn files_action(&mut self, action: &str, path: &str) -> Result<()> {
        if !cfg!(any(target_os = "android", target_os = "ios")) || !path.trim().is_empty() {
            return self.files(action, path);
        }
        if action.starts_with("import-") {
            return crate::platform::pick_document(self.page, action);
        }
        if action == "export-current" {
            let m = self
                .current
                .as_ref()
                .context("Play a local audio file first")?;
            if m.is_remote() {
                bail!("Download this audio before exporting it");
            }
            let extension = Path::new(&m.location)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("audio");
            let name = format!(
                "{}.{}",
                m.title.replace(['/', '\\', '\r', '\n', ':'], "_"),
                extension
            );
            return crate::platform::export_document(&m.location, &name);
        }
        let extension = match action {
            "export-m3u" => "m3u",
            "export-radio-m3u" => "m3u8",
            "export-opml" => "opml",
            "export-lyrics" => "lrc",
            "export-sync" => "sync",
            _ => "json",
        };
        let dir = self.base.join("exports");
        std::fs::create_dir_all(&dir)?;
        let output = dir.join(format!(
            "Auvemir-{}-{:016x}.{extension}",
            action.trim_start_matches("export-"),
            rand::random::<u64>()
        ));
        self.files(action, &output.to_string_lossy())?;
        if action == "export-sync" {
            self.pending_export = Some(output);
        } else {
            crate::platform::export_document(
                &output.to_string_lossy(),
                output
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Auvemir export"),
            )?;
        }
        Ok(())
    }
    fn files(&mut self, action: &str, path: &str) -> Result<()> {
        if path.trim().is_empty() {
            bail!("Enter a file path first");
        }
        match action {
            "export-current" => {
                let media = self
                    .current
                    .as_ref()
                    .context("Play a local audio file first")?;
                if media.is_remote() {
                    bail!("Download this audio before exporting it");
                }
                let mut input = std::fs::File::open(&media.location)?;
                let mut output = std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(path)
                    .context("Export destination must be a new file")?;
                std::io::copy(&mut input, &mut output)?;
                output.sync_all()?;
            }
            "import-sync" | "export-sync" => {
                if self.sync_busy {
                    bail!("Sync is already running");
                }
                let secret: auvemir_core::sync::Secret =
                    serde_json::from_str(&crate::credentials::load("sync-webdav")?)?;
                let path = PathBuf::from(path);
                let import = action == "import-sync";
                let snapshot = self.db.sync_snapshot()?;
                self.sync_busy = true;
                self.status = "Processing encrypted sync file…".into();
                self.job(async move {
                    let result = tokio::task::spawn_blocking(
                        move || -> Result<auvemir_core::sync::Snapshot> {
                            if import {
                                if std::fs::metadata(&path)?.len()
                                    > auvemir_core::sync::MAX_BYTES as u64
                                {
                                    bail!("Sync file exceeds 16 MiB");
                                }
                                auvemir_core::sync::decrypt(
                                    &std::fs::read(path)?,
                                    &secret.passphrase,
                                )
                            } else {
                                atomic_export(
                                    &path,
                                    &auvemir_core::sync::encrypt(&snapshot, &secret.passphrase)?,
                                )?;
                                Ok(snapshot)
                            }
                        },
                    )
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.map_err(|e| e.to_string()));
                    Ok(Command::SyncFinished(result))
                });
                return Ok(());
            }
            "import-history" => {
                if std::fs::metadata(path)?.len() > 32 * 1024 * 1024 {
                    bail!("History file exceeds 32 MiB");
                }
                let listens: Vec<auvemir_core::scrobble::Listen> =
                    serde_json::from_slice(&std::fs::read(path)?)?;
                if listens.len() > 100000 {
                    bail!("History import exceeds 100,000 listens");
                }
                for listen in listens {
                    self.db.record_listen(&listen, &[])?;
                }
            }
            "export-history" => atomic_export(
                Path::new(path),
                &serde_json::to_vec_pretty(&self.db.listening_history()?)?,
            )?,
            "import-lyrics" => {
                let m = self.current.as_ref().context("Play a track first")?;
                if std::fs::metadata(path)?.len() > 2 * 1024 * 1024 {
                    bail!("Lyrics file exceeds 2 MiB");
                }
                self.handle(Command::Lyrics(
                    m.id.clone(),
                    std::fs::read_to_string(path)?,
                ))?;
            }
            "export-lyrics" => {
                let m = self.current.as_ref().context("Play a track first")?;
                atomic_export(
                    Path::new(path),
                    self.db
                        .lyrics(&m.id)?
                        .context("Load lyrics first")?
                        .as_bytes(),
                )?;
            }
            "export-m3u" => atomic_export(
                Path::new(path),
                auvemir_core::library::write_m3u(&self.queue.items).as_bytes(),
            )?,
            "import-m3u" => {
                let items =
                    auvemir_core::library::read_m3u(Path::new(path), MediaKind::LocalMusic)?;
                self.db.upsert(&items)?;
                self.queue.replace(items, 0);
                self.persist_queue()?;
                self.page = 8;
                self.refresh()?;
            }
            "export-opml" => atomic_export(
                Path::new(path),
                auvemir_core::providers::export_opml(&self.db.subscriptions()?).as_bytes(),
            )?,
            "import-opml" => {
                let urls = auvemir_core::providers::import_opml(&std::fs::read_to_string(path)?)?;
                let p = self.providers()?;
                for url in urls {
                    let p = p.clone();
                    self.view_job(async move {
                        let (title, items) = p.feed(&url, MediaKind::Podcast).await?;
                        Ok(Command::Feed(url, title, MediaKind::Podcast, items))
                    });
                }
            }
            "export-radio-m3u" => atomic_export(
                Path::new(path),
                auvemir_core::radio::write_m3u(&self.db.stations()?).as_bytes(),
            )?,
            "import-radio-m3u" => {
                if std::fs::metadata(path)?.len() > 8 * 1024 * 1024 {
                    bail!("Radio playlist exceeds 8 MiB");
                }
                let stations = auvemir_core::radio::parse_m3u(&std::fs::read_to_string(path)?)?;
                for s in stations {
                    self.db.save_station(&s)?;
                }
                self.page = 5;
                self.query.clear();
                self.refresh()?;
            }
            "export-radio" => atomic_export(
                Path::new(path),
                &serde_json::to_vec_pretty(&self.db.stations()?)?,
            )?,
            "import-radio" => {
                let stations: Vec<Station> = serde_json::from_slice(&std::fs::read(path)?)?;
                for mut s in stations {
                    s.custom = true;
                    s.id = format!("custom:{}", stable_id("import-radio", &s.url));
                    self.db.save_station(&s)?;
                }
                self.page = 5;
                self.query.clear();
                self.refresh()?;
            }
            _ => bail!("Unknown file operation"),
        }
        self.status = "File operation completed.".into();
        Ok(())
    }
}
fn atomic_export(path: &Path, bytes: &[u8]) -> Result<()> {
    // Require a fresh destination to avoid overwriting a user's unrelated file.
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .context("Export destination must be a new file")?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
fn kind_for_page(page: i32) -> Option<MediaKind> {
    match page {
        1 => Some(MediaKind::LocalMusic),
        2 => Some(MediaKind::OnlineMusic),
        3 => Some(MediaKind::Audiobook),
        4 => Some(MediaKind::Podcast),
        5 => Some(MediaKind::Radio),
        6 => Some(MediaKind::Ambient),
        7 => Some(MediaKind::Recording),
        _ => None,
    }
}
fn headings(page: i32) -> (&'static str, &'static str) {
    match page {
        1 => ("Your library", "Your music, always close."),
        2 => (
            "Discover",
            "Explore your personal servers and online catalogs.",
        ),
        3 => ("Audiobooks", "Stories worth slowing down for."),
        4 => ("Podcasts", "Follow the voices you love."),
        5 => (
            "Radio",
            "A world of live sound. Your stations, your choice.",
        ),
        6 => (
            "Ambient",
            "Build a quieter moment with layered loops and generated noise.",
        ),
        7 => ("Recordings", "Capture a thought. Keep it on your device."),
        8 => ("Queue", "What comes next."),
        9 => ("Search", "Find something in your library."),
        10 => ("Favorites", "The audio you come back to."),
        11 => ("Settings", "Make Auvemir yours."),
        _ => ("Your listening room", "A little space for what moves you."),
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;

    struct StateDirectory(PathBuf);
    impl Drop for StateDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn worker_fixture() -> (StateDirectory, Worker) {
        let directory = StateDirectory(
            std::env::temp_dir().join(format!("auvemir-state-{:016x}", rand::random::<u64>())),
        );
        std::fs::create_dir(&directory.0).unwrap();
        let (tx, _rx) = mpsc::sync_channel(64);
        let worker = Worker::new(directory.0.clone(), tx, Default::default()).unwrap();
        (directory, worker)
    }
    #[test]
    fn late_catalog_and_errors_do_not_replace_current_screen() {
        let (_directory, mut worker) = worker_fixture();
        worker.page = 1;
        worker.view_generation = 2;
        worker.status = "Current".into();
        let book = CatalogBook {
            id: "late".into(),
            title: "Late".into(),
            author: "A".into(),
            language: "English".into(),
            feed_url: "https://example.test/feed".into(),
            cover: None,
        };
        worker
            .handle(Command::ViewResult(
                1,
                Box::new(Command::Catalog(vec![book])),
            ))
            .unwrap();
        worker
            .handle(Command::ViewResult(
                1,
                Box::new(Command::Notice("Old error".into())),
            ))
            .unwrap();
        assert!(worker.collections.is_empty());
        assert_eq!(worker.page, 1);
        assert_eq!(worker.status, "Current");
    }
    #[test]
    fn subscription_completion_after_navigation_persists_without_jumping() {
        let (_directory, mut worker) = worker_fixture();
        worker.page = 1;
        worker.view_generation = 2;
        let item = Media::new(
            MediaKind::Podcast,
            "rss",
            "https://example.test/episode.mp3",
            "Episode",
        );
        worker
            .handle(Command::ViewResult(
                1,
                Box::new(Command::Feed(
                    "https://example.test/feed".into(),
                    "Feed".into(),
                    MediaKind::Podcast,
                    vec![item.clone()],
                )),
            ))
            .unwrap();
        assert_eq!(worker.page, 1);
        assert_eq!(worker.db.get(&item.id).unwrap(), Some(item));
        assert_eq!(worker.db.subscriptions().unwrap().len(), 1);
    }
    #[test]
    fn canceled_import_ignores_already_queued_batches_and_completion() {
        let (_directory, mut worker) = worker_fixture();
        worker.scan_generation = 1;
        worker.scanning = true;
        worker.action("cancel-import").unwrap();
        let status = worker.status.clone();
        let item = Media::new(MediaKind::LocalMusic, "local", "/late.flac", "Late");
        worker
            .handle(Command::Scanned(
                1,
                auvemir_core::library::Scanned {
                    items: vec![item],
                    fingerprints: vec![],
                    errors: vec![],
                },
            ))
            .unwrap();
        worker.handle(Command::ScanFinished(1, Ok(()))).unwrap();
        assert_eq!(worker.db.count().unwrap(), 0);
        assert_eq!(worker.status, status);
        assert!(!worker.scanning);
    }
    #[test]
    fn scan_database_failure_cancels_and_cannot_report_success() {
        let (_directory, mut worker) = worker_fixture();
        worker.scan_generation = 4;
        worker.scanning = true;
        let item = Media::new(MediaKind::LocalMusic, "local", "/failed.flac", "Failed");
        assert!(
            worker
                .handle(Command::Scanned(
                    4,
                    auvemir_core::library::Scanned {
                        items: vec![item],
                        fingerprints: vec![("/failed.flac".into(), 1, u64::MAX)],
                        errors: vec![]
                    }
                ))
                .is_err()
        );
        assert!(!worker.scanning);
        assert!(worker.scan_cancel.load(Ordering::Relaxed));
        assert_eq!(worker.db.count().unwrap(), 0);
        worker.handle(Command::ScanFinished(4, Ok(()))).unwrap();
        assert!(!worker.status.contains("Import complete"));
    }
    #[test]
    fn preferences_and_removed_connection_survive_restart() {
        let (directory, mut worker) = worker_fixture();
        worker
            .handle(Command::Preference("appearance-light".into(), true))
            .unwrap();
        worker
            .handle(Command::Preference("motion-reduced".into(), true))
            .unwrap();
        worker.handle(Command::Sort(2)).unwrap();
        worker.action("disconnect:ytmusic").unwrap();
        drop(worker);
        let (tx, _rx) = mpsc::sync_channel(64);
        let worker = Worker::new(directory.0.clone(), tx, Default::default()).unwrap();
        assert_eq!(worker.theme_index, 4);
        assert!(worker.reduce_motion);
        assert_eq!(worker.sort, MediaSort::Artist);
        assert!(worker.servers.is_empty());
        assert!(worker.active_server.is_none());
    }

    #[test]
    fn webdav_folder_navigation_round_trips_encoded_paths_and_parent() {
        let path = "/Music/Live & Studio/";
        let id = webdav_directory_id("dav", path);
        assert_eq!(
            parse_webdav_directory_id(&id).unwrap(),
            ("dav".into(), path.into())
        );
        assert_eq!(webdav_parent_path(path).as_deref(), Some("/Music/"));
        assert_eq!(webdav_parent_path("/Music/"), Some("/".into()));
        assert_eq!(webdav_parent_path("/"), None);
    }

    #[test]
    fn audiobookshelf_resume_maps_book_time_to_track_and_offset() {
        let first = Media::new(
            MediaKind::Audiobook,
            "server:abs",
            "auvemir://server/abs/book?track=0",
            "First",
        );
        let mut second = Media::new(
            MediaKind::Audiobook,
            "server:abs",
            "auvemir://server/abs/book?track=1",
            "Second",
        );
        let mut first = first;
        first.duration_ms = 120_000;
        second.duration_ms = 180_000;
        assert_eq!(
            audiobook_resume_offset(
                &[first.clone(), second.clone()],
                Some(auvemir_core::servers::RemoteProgress {
                    current_time_ms: 150_000,
                    is_finished: false,
                }),
            ),
            (1, 30_000)
        );
        assert_eq!(
            audiobook_resume_offset(
                &[first.clone(), second.clone()],
                Some(auvemir_core::servers::RemoteProgress {
                    current_time_ms: 300_000,
                    is_finished: false,
                }),
            ),
            (0, 0)
        );
        assert_eq!(
            audiobook_resume_offset(
                &[first, second],
                Some(auvemir_core::servers::RemoteProgress {
                    current_time_ms: 150_000,
                    is_finished: true,
                }),
            ),
            (0, 0)
        );
    }

    #[test]
    fn late_lyrics_cannot_overwrite_edits_or_the_current_track() {
        let directory = StateDirectory(
            std::env::temp_dir().join(format!("auvemir-lyrics-{:016x}", rand::random::<u64>())),
        );
        std::fs::create_dir(&directory.0).unwrap();
        let (tx, _rx) = mpsc::sync_channel(64);
        let mut worker = Worker::new(directory.0.clone(), tx, Default::default()).unwrap();
        let first = Media::new(MediaKind::LocalMusic, "local", "first.wav", "First");
        let second = Media::new(MediaKind::LocalMusic, "local", "second.wav", "Second");
        worker.db.upsert(&[first.clone(), second.clone()]).unwrap();
        worker.current = Some(first.clone());
        worker.lyric_requests.insert(first.id.clone(), 1);
        worker
            .handle(Command::SaveLyrics(
                first.id.clone(),
                "[00:02.00]Manual edit".into(),
            ))
            .unwrap();
        worker
            .handle(Command::LyricsFetched(
                first.id.clone(),
                1,
                Ok("[00:01.00]Late network text".into()),
            ))
            .unwrap();
        assert_eq!(worker.lyrics_text, "[00:02.00]Manual edit");
        assert_eq!(worker.lyric_lines[0].ms, 2_000);
        worker.lyric_requests.insert(second.id.clone(), 2);
        worker
            .handle(Command::LyricsFetched(
                second.id.clone(),
                2,
                Ok("[00:03.00]Second track".into()),
            ))
            .unwrap();
        assert_eq!(worker.lyrics_text, "[00:02.00]Manual edit");
        assert!(
            worker
                .handle(Command::SeekLyric(second.id.clone(), 0))
                .is_err()
        );
        assert!(
            worker
                .handle(Command::SeekLyric(first.id.clone(), -1))
                .is_err()
        );
        assert!(
            worker
                .handle(Command::SaveLyrics(
                    first.id.clone(),
                    "x".repeat(2 * 1024 * 1024 + 1)
                ))
                .is_err()
        );
        worker.action("lyrics-later").unwrap();
        drop(worker);
        let db = Store::open(directory.0.join("auvemir.sqlite")).unwrap();
        assert_eq!(
            db.lyrics(&first.id).unwrap().unwrap(),
            "[00:02.00]Manual edit"
        );
        assert_eq!(
            db.lyrics(&second.id).unwrap().unwrap(),
            "[00:03.00]Second track"
        );
        assert_eq!(db.setting("lyrics-offset").unwrap().unwrap(), "500");
    }
}
