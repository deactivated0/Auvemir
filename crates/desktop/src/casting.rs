use anyhow::{Result, bail};
use auvemir_core::{
    cast::{Device, LocalFile, Session, Status, mime},
    model::{Media, MediaKind},
};
use std::{path::PathBuf, time::Duration};
pub enum Event {
    Loaded(u64, Box<Media>, Status),
    Status(u64, Status),
    Error(String),
}
enum Command {
    Load(u64, Box<Media>, u64, Option<auvemir_core::servers::Stream>),
    Control(&'static str, f64),
}
enum Relay {
    File {
        _file: LocalFile,
    },
    Remote {
        _remote: auvemir_core::cast_proxy::RemoteAudio,
    },
}
pub struct Handle {
    tx: tokio::sync::mpsc::Sender<Command>,
    cancel: tokio::sync::watch::Sender<bool>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
    }
}
impl Handle {
    pub fn start(
        runtime: &tokio::runtime::Runtime,
        device: Device,
        notify: impl Fn(Event) + Send + Sync + 'static,
    ) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
        let task=runtime.spawn(async move{
            let connection=tokio::select!{_ = cancelled.changed()=>return,result=Session::connect(&device)=>result};
            let mut session=match connection{Ok(s)=>s,Err(e)=>{notify(Event::Error(e.to_string()));return}};
            let mut source=None;let mut generation=0;let mut loaded=false;let mut timer=tokio::time::interval(Duration::from_secs(1));timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop{
                let operation=tokio::select!{
                    _=cancelled.changed()=>break,
                    command=rx.recv()=>command,
                    _=timer.tick(),if loaded=>{
                        let result=tokio::select!{_ = cancelled.changed()=>break,result=session.status()=>result};
                        match result{Ok(status)=>notify(Event::Status(generation,status)),Err(e)=>{notify(Event::Error(e.to_string()));break;}}continue;
                    }
                };
                let Some(command)=operation else{break};
                let result=tokio::select!{_ = cancelled.changed()=>break,result=async{
                    match command{
                        Command::Load(g,m,position,upstream)=>{
                            let m=*m;
                            let (url,content_type,next_source)=if let Some(upstream)=upstream{
                                let relay=auvemir_core::cast_proxy::RemoteAudio::serve(upstream.location,upstream.client,device.address.ip()).await?;
                                (relay.url.clone(),relay.content_type.clone(),Some(Relay::Remote{_remote:relay}))
                            }else if m.location.starts_with("http://")||m.location.starts_with("https://"){
                                auvemir_core::network::validate_url(&m.location,false)?;
                                (m.location.clone(),mime(std::path::Path::new(url::Url::parse(&m.location)?.path())).to_owned(),None)
                            }else if m.location.starts_with("auvemir://"){bail!("Resolve this server item before casting it");}
                            else {let path=PathBuf::from(&m.location);let content_type=mime(&path).to_owned();let server=LocalFile::serve(path,device.address.ip()).await?;(server.url.clone(),content_type,Some(Relay::File{_file:server}))};
                            let status=session.load(&url,&content_type,&m.title,&m.artist,position,m.kind==MediaKind::Radio).await?;source=next_source;generation=g;loaded=true;notify(Event::Loaded(g,Box::new(m),status));
                        },
                        Command::Control(command,value)=>{let status=session.control(command,value).await?;if command=="stop"{loaded=false;source=None;}notify(Event::Status(generation,status));},
                    }Ok::<_,anyhow::Error>(())
                }=>result};
                if let Err(e)=result{notify(Event::Error(e.to_string()));break;}
            }
            let _=tokio::time::timeout(Duration::from_secs(2),session.control("stop",0.)).await;
            drop(source);
        });
        Self {
            tx,
            cancel,
            task: Some(task),
        }
    }
    pub fn load(&self, g: u64, media: Media, position: u64) -> Result<()> {
        self.tx
            .try_send(Command::Load(g, Box::new(media), position, None))
            .map_err(|_| anyhow::anyhow!("Cast command queue is busy or closed"))
    }
    pub fn load_remote(
        &self,
        g: u64,
        media: Media,
        position: u64,
        source: auvemir_core::servers::Stream,
    ) -> Result<()> {
        self.tx
            .try_send(Command::Load(g, Box::new(media), position, Some(source)))
            .map_err(|_| anyhow::anyhow!("Cast command queue is busy or closed"))
    }
    pub fn control(&self, command: &'static str, value: f64) -> Result<()> {
        self.tx
            .try_send(Command::Control(command, value))
            .map_err(|_| anyhow::anyhow!("Cast command queue is busy or closed"))
    }
    pub async fn close(mut self) {
        self.cancel.send(true).ok();
        if let Some(mut task) = self.task.take()
            && tokio::time::timeout(Duration::from_secs(3), &mut task)
                .await
                .is_err()
        {
            task.abort();
        }
    }
}
