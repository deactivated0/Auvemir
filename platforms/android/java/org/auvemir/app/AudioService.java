package org.auvemir.app;

import android.app.*;
import android.content.*;
import android.content.pm.ServiceInfo;
import android.content.pm.PackageManager;
import java.util.concurrent.*;
import android.database.Cursor;
import android.database.sqlite.SQLiteDatabase;
import android.media.*;
import android.media.browse.MediaBrowser;
import android.media.session.*;
import android.os.*;
import android.service.media.MediaBrowserService;
import java.io.File;
import java.util.*;

/** Native MediaSession and foreground lifetime; decoding and the queue remain in Rust. */
public final class AudioService extends MediaBrowserService {
    private static final Object LOCK=new Object();
    private static final Handler MAIN=new Handler(Looper.getMainLooper());
    private static AudioService instance;
    private static final String INTERNAL=UUID.randomUUID().toString();
    private final ThreadPoolExecutor reads=new ThreadPoolExecutor(1,1,30,TimeUnit.SECONDS,new ArrayBlockingQueue<Runnable>(8),new ThreadPoolExecutor.AbortPolicy());
    private boolean startedByApp;
    private final FocusState focusState=new FocusState();
    private String sessionMetadata="";
    public static void userTransport(){MAIN.post(()->{
        AudioService service=instance;if(service==null)return;
        service.focusState.userTransport();
        // An already-paused session may have no metadata update to trigger refresh().
        if(!playing&&!recording&&service.focusState.shouldRelease()){
            service.audio.abandonAudioFocusRequest(service.focus);service.focusState.released();
        }
    });}
    private static volatile boolean recording,playing;
    private static int recordingState;
    private static boolean canSeek;
    private static float speed=1f;
    private static String mediaId="",title="",artist="";
    private static long duration,position,lastSecond=-1;
    private static String lastMetadata="";
    private MediaSession session;private AudioManager audio;private AudioFocusRequest focus;
    private PowerManager.WakeLock wake;
    private final BroadcastReceiver noisy=new BroadcastReceiver(){@Override public void onReceive(Context context,Intent intent){AuvemirActivity.command("pause");}};
    public static void activate(Context context,boolean capture){
        synchronized(LOCK){if(capture){recording=true;recordingState=1;}}
        Intent intent=new Intent(context,AudioService.class).putExtra("auvemir.internal",INTERNAL);context.startForegroundService(intent);
    }
    public static void setRecording(int state){boolean changed;synchronized(LOCK){changed=recordingState!=state;recordingState=state;recording=state==1;}if(changed)MAIN.post(()->{if(instance!=null)instance.refresh(true);});}
    public static void update(Context context,String id,String t,String a,long d,long p,boolean active,float rate,boolean seekable){
        boolean changed;synchronized(LOCK){String metadata=id+'\0'+t+'\0'+a+'\0'+d+'\0'+rate+'\0'+seekable;changed=active!=playing||p/1000!=lastSecond||!metadata.equals(lastMetadata);mediaId=id;title=t;artist=a;duration=d;position=p;playing=active;speed=rate;canSeek=seekable;lastSecond=p/1000;lastMetadata=metadata;}
        if(changed)MAIN.post(()->{if(instance!=null)instance.refresh(false);});
    }
    @Override public void onCreate(){
        super.onCreate();reads.allowCoreThreadTimeOut(true);instance=this;((AuvemirApplication)getApplication()).ensureEngine();audio=(AudioManager)getSystemService(AUDIO_SERVICE);
        NotificationChannel channel=new NotificationChannel("playback","Playback and recording",NotificationManager.IMPORTANCE_LOW);channel.setShowBadge(false);getSystemService(NotificationManager.class).createNotificationChannel(channel);
        wake=((PowerManager)getSystemService(POWER_SERVICE)).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK,"Auvemir:audio");wake.setReferenceCounted(false);
        focus=new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN).setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build()).setOnAudioFocusChangeListener(change->{
            if(change==AudioManager.AUDIOFOCUS_GAIN){if(focusState.gain())AuvemirActivity.command("focus-play");}
            else if(change==AudioManager.AUDIOFOCUS_LOSS){focusState.loss(false,playing);AuvemirActivity.command("focus-pause");if(recording)AuvemirActivity.command("pause-recording");}
            else if(change==AudioManager.AUDIOFOCUS_LOSS_TRANSIENT||change==AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK){focusState.loss(true,playing);AuvemirActivity.command("focus-pause");if(recording)AuvemirActivity.command("pause-recording");}
        },MAIN).build();
        session=new MediaSession(this,"Auvemir");session.setFlags(MediaSession.FLAG_HANDLES_MEDIA_BUTTONS|MediaSession.FLAG_HANDLES_TRANSPORT_CONTROLS);
        session.setCallback(new MediaSession.Callback(){
            @Override public void onPlay(){AuvemirActivity.command("play");}
            @Override public void onPause(){AuvemirActivity.command("pause");}
            @Override public void onStop(){AuvemirActivity.command("stop");}
            @Override public void onSkipToNext(){AuvemirActivity.command("next");}
            @Override public void onSkipToPrevious(){AuvemirActivity.command("previous");}
            @Override public void onSeekTo(long p){AuvemirActivity.nativeCommand("seek","",p);}
            @Override public void onPlayFromMediaId(String id,Bundle extras){AuvemirActivity.nativeCommand("play-id",id,0);}
            @Override public void onPlayFromSearch(String query,Bundle extras){try{reads.execute(()->{List<MediaBrowser.MediaItem> matches=search(query);if(!matches.isEmpty())MAIN.post(()->{if(instance==AudioService.this)onPlayFromMediaId(matches.get(0).getMediaId(),extras);});});}catch(RejectedExecutionException ignored){}}
        },MAIN);
        session.setSessionActivity(PendingIntent.getActivity(this,0,new Intent(this,AuvemirActivity.class),PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE));session.setActive(true);setSessionToken(session.getSessionToken());
        if(Build.VERSION.SDK_INT>=33)registerReceiver(noisy,new IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY),Context.RECEIVER_NOT_EXPORTED);else registerReceiver(noisy,new IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY));
    }
    @Override public int onStartCommand(Intent intent,int flags,int id){if(intent==null||!INTERNAL.equals(intent.getStringExtra("auvemir.internal"))){if(!startedByApp)stopSelf(id);return START_NOT_STICKY;}startedByApp=true;refresh(true);if(intent!=null){String action=intent.getAction();if(Arrays.asList("play","pause","next","previous","stop","pause-recording","resume-recording","stop-recording").contains(action))AuvemirActivity.command(action);}return START_NOT_STICKY;}
    private PendingIntent action(String command,int code){return PendingIntent.getService(this,code,new Intent(this,AudioService.class).setAction(command).putExtra("auvemir.internal",INTERNAL),PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE);}
    private void refresh(boolean foreground){
        final boolean capture,active,seekable,hasCapture;final String t,a,id;final long d,p;final float rate;
        synchronized(LOCK){capture=recording;hasCapture=recordingState!=0;active=playing;t=title;a=artist;id=mediaId;d=duration;p=position;rate=speed;seekable=canSeek;}
        long actions=PlaybackState.ACTION_PLAY|PlaybackState.ACTION_PAUSE|PlaybackState.ACTION_PLAY_PAUSE|PlaybackState.ACTION_STOP|PlaybackState.ACTION_SKIP_TO_NEXT|PlaybackState.ACTION_SKIP_TO_PREVIOUS|PlaybackState.ACTION_PLAY_FROM_MEDIA_ID|PlaybackState.ACTION_PLAY_FROM_SEARCH;
        if(seekable&&d>0)actions|=PlaybackState.ACTION_SEEK_TO;
        String metadata=id+'\0'+t+'\0'+a+'\0'+d;
        if(!metadata.equals(sessionMetadata)){session.setMetadata(new MediaMetadata.Builder().putString(MediaMetadata.METADATA_KEY_MEDIA_ID,id).putString(MediaMetadata.METADATA_KEY_TITLE,t).putString(MediaMetadata.METADATA_KEY_ARTIST,a).putLong(MediaMetadata.METADATA_KEY_DURATION,d).build());sessionMetadata=metadata;}
        session.setPlaybackState(new PlaybackState.Builder().setActions(actions).setState(id.isEmpty()?PlaybackState.STATE_STOPPED:active?PlaybackState.STATE_PLAYING:PlaybackState.STATE_PAUSED,p,active?rate:0f,SystemClock.elapsedRealtime()).build());
        Notification.Builder notificationBuilder=new Notification.Builder(this,"playback").setSmallIcon(org.auvemir.app.R.drawable.ic_auvemir).setContentTitle(hasCapture?(capture?"Recording microphone":"Recording paused"):t.isEmpty()?"Auvemir":t).setContentText(a).setContentIntent(session.getController().getSessionActivity()).setVisibility(Notification.VISIBILITY_PUBLIC).setOnlyAlertOnce(true).setOngoing(active||capture);
        if(hasCapture){
            notificationBuilder.addAction(new Notification.Action.Builder(null,capture?"Pause recording":"Resume recording",action(capture?"pause-recording":"resume-recording",4)).build()).addAction(new Notification.Action.Builder(null,"Stop recording",action("stop-recording",5)).build());
        }else{
            notificationBuilder.addAction(new Notification.Action.Builder(null,"Previous",action("previous",1)).build()).addAction(new Notification.Action.Builder(null,active?"Pause":"Play",action(active?"pause":"play",2)).build()).addAction(new Notification.Action.Builder(null,"Stop",action("stop",3)).build());
        }
        Notification notification=notificationBuilder.setStyle(new Notification.MediaStyle().setMediaSession(session.getSessionToken()).setShowActionsInCompactView(hasCapture?new int[]{0,1}:new int[]{0,1,2})).build();
        int types=ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK;if(capture)types|=ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE;
        if(Build.VERSION.SDK_INT>=29)startForeground(1,notification,types);else startForeground(1,notification);
        // Android 15+ requires foreground eligibility before an audio-focus request.
        if(active||capture){if(!focusState.granted){focusState.requestResult(audio.requestAudioFocus(focus)==AudioManager.AUDIOFOCUS_REQUEST_GRANTED);if(!focusState.granted){audio.abandonAudioFocusRequest(focus);if(active)AuvemirActivity.command("focus-pause");if(capture)AuvemirActivity.command("pause-recording");}}if(!wake.isHeld())wake.acquire();}else {if(focusState.shouldRelease()){audio.abandonAudioFocusRequest(focus);focusState.released();}if(wake.isHeld())wake.release();}
        if(!active&&!capture&&!foreground)stopForeground(STOP_FOREGROUND_DETACH);
    }
    @Override public BrowserRoot onGetRoot(String packageName,int uid,Bundle hints){
        PackageManager pm=getPackageManager();String[] packages=pm.getPackagesForUid(uid);
        if(packages==null||!Arrays.asList(packages).contains(packageName))return null;
        boolean trusted=uid==getApplicationInfo().uid||uid==android.os.Process.SYSTEM_UID||checkPermission("android.permission.MEDIA_CONTENT_CONTROL",-1,uid)==PackageManager.PERMISSION_GRANTED;
        if(!trusted&&Arrays.asList("com.google.android.projection.gearhead","com.google.android.googlequicksearchbox").contains(packageName))trusted=pm.checkSignatures(packageName,"com.google.android.gms")==PackageManager.SIGNATURE_MATCH;
        return trusted?new BrowserRoot("root",null):null;
    }
    @Override public void onLoadChildren(String parent,Result<List<MediaBrowser.MediaItem>> result){
        result.detach();
        try{reads.execute(()->{
        ArrayList<MediaBrowser.MediaItem> items=new ArrayList<>();
        if(parent.equals("root")){
            String[][] groups={{"local_music","Music"},{"audiobook","Audiobooks"},{"podcast","Podcasts"},{"radio","Radio"},{"recording","Recordings"},{"online_music","Online music"}};
            for(String[] group:groups)items.add(new MediaBrowser.MediaItem(new MediaDescription.Builder().setMediaId("category:"+group[0]+":0").setTitle(group[1]).build(),MediaBrowser.MediaItem.FLAG_BROWSABLE));
        }else if(parent.startsWith("category:")){
            String[] parts=parent.split(":");if(parts.length==3&&parts[2].matches("[0-9]{1,7}")){
                int offset=Integer.parseInt(parts[2]);items.addAll(query("kind=?",new String[]{parts[1]},offset));
                if(items.size()==100)items.add(new MediaBrowser.MediaItem(new MediaDescription.Builder().setMediaId("category:"+parts[1]+":"+(offset+100)).setTitle("More").build(),MediaBrowser.MediaItem.FLAG_BROWSABLE));
            }
        }
        result.sendResult(items);
        });}catch(RejectedExecutionException error){result.sendResult(Collections.emptyList());}
    }
    private List<MediaBrowser.MediaItem> search(String query){String pattern="%"+query.replace("\\","\\\\").replace("%","\\%").replace("_","\\_")+"%";return query("(title LIKE ? ESCAPE '\\' OR artist LIKE ? ESCAPE '\\')",new String[]{pattern,pattern},0);}
    private List<MediaBrowser.MediaItem> query(String selection,String[] args,int offset){
        ArrayList<MediaBrowser.MediaItem> items=new ArrayList<>();File file=new File(getFilesDir(),"auvemir.sqlite");if(!file.isFile())return items;
        try(SQLiteDatabase db=SQLiteDatabase.openDatabase(file.getAbsolutePath(),null,SQLiteDatabase.OPEN_READONLY|SQLiteDatabase.NO_LOCALIZED_COLLATORS);Cursor rows=db.query("media",new String[]{"id","title","artist"},selection,args,null,null,"album COLLATE NOCASE,track_number,title COLLATE NOCASE",offset+",100")){
            while(rows.moveToNext())items.add(new MediaBrowser.MediaItem(new MediaDescription.Builder().setMediaId(rows.getString(0)).setTitle(rows.getString(1)).setSubtitle(rows.getString(2)).build(),MediaBrowser.MediaItem.FLAG_PLAYABLE));
        }catch(RuntimeException ignored){}return items;
    }
    @Override public void onDestroy(){instance=null;reads.shutdownNow();AuvemirActivity.command("service-stopped");unregisterReceiver(noisy);audio.abandonAudioFocusRequest(focus);if(wake.isHeld())wake.release();session.release();super.onDestroy();}
}
