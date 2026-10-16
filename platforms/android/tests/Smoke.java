package org.auvemir.tests;

import android.app.*;
import android.content.*;
import android.graphics.Bitmap;
import android.view.accessibility.AccessibilityNodeInfo;
import android.media.browse.MediaBrowser;
import android.media.AudioManager;
import android.media.AudioFocusRequest;
import android.media.AudioAttributes;
import android.media.session.*;
import android.os.*;
import java.io.*;
import java.lang.reflect.Method;
import java.nio.*;
import java.util.*;
import java.util.concurrent.*;
import org.json.*;

/** Instrumentation runs inside the target UID; production has no test-only command endpoints. */
public final class Smoke extends Instrumentation {
    private final ArrayList<String> steps=new ArrayList<>();
    private boolean systemUiDialogSeen;
    private MediaBrowser browser;
    private MediaController controller;
    private Method command;
    @Override public void onCreate(Bundle args){super.onCreate(args);start();}
    private void step(String value){steps.add(value);Bundle status=new Bundle();status.putString("step",value);sendStatus(0,status);}
    private void check(boolean value,String message){if(!value)throw new AssertionError(message);}
    private void nativeCommand(String action,String value,long number)throws Exception{command.invoke(null,action,value,number);}
    private PlaybackState state(){return controller.getPlaybackState();}
    private void awaitState(int expected)throws Exception{
        long deadline=SystemClock.elapsedRealtime()+45000;
        while(SystemClock.elapsedRealtime()<deadline){PlaybackState s=state();if(s!=null&&s.getState()==expected)return;Thread.sleep(200);}
        throw new AssertionError("Expected playback state "+expected+", got "+(state()==null?"none":state().getState()));
    }
    private File fixture()throws Exception{
        File file=new File(getTargetContext().getFilesDir(),"Auvemir-smoke.wav");
        int bytes=22050*2*180;
        ByteBuffer header=ByteBuffer.allocate(44).order(ByteOrder.LITTLE_ENDIAN);
        header.put("RIFF".getBytes("US-ASCII")).putInt(36+bytes).put("WAVEfmt ".getBytes("US-ASCII")).putInt(16).putShort((short)1).putShort((short)1).putInt(22050).putInt(44100).putShort((short)2).putShort((short)16).put("data".getBytes("US-ASCII")).putInt(bytes);
        try(FileOutputStream out=new FileOutputStream(file)){out.write(header.array());byte[] second=new byte[44100];for(int i=0;i<180;i++)out.write(second);out.getFD().sync();}
        return file;
    }
    private void connect()throws Exception{
        CountDownLatch connected=new CountDownLatch(1);boolean[] ok={false};
        runOnMainSync(()->{
            browser=new MediaBrowser(getTargetContext(),new ComponentName("org.auvemir.app","org.auvemir.app.AudioService"),new MediaBrowser.ConnectionCallback(){
                @Override public void onConnected(){ok[0]=true;connected.countDown();}
                @Override public void onConnectionFailed(){connected.countDown();}
            },null);browser.connect();
        });
        check(connected.await(45,TimeUnit.SECONDS)&&ok[0],"Media browser could not connect without an Activity");
        runOnMainSync(()->controller=new MediaController(getTargetContext(),browser.getSessionToken()));
    }
    private String importedTrack()throws Exception{
        long deadline=SystemClock.elapsedRealtime()+45000;
        while(SystemClock.elapsedRealtime()<deadline){
            CountDownLatch done=new CountDownLatch(1);String[] id={null};
            runOnMainSync(()->{
                browser.unsubscribe("category:local_music:0");
                browser.subscribe("category:local_music:0",new MediaBrowser.SubscriptionCallback(){
                    @Override public void onChildrenLoaded(String parent,List<MediaBrowser.MediaItem> children){
                        for(MediaBrowser.MediaItem item:children){CharSequence title=item.getDescription().getTitle();if(item.isPlayable()&&title!=null&&title.toString().contains("Auvemir-smoke")){id[0]=item.getMediaId();break;}}done.countDown();
                    }
                    @Override public void onError(String parent){done.countDown();}
                });
            });
            check(done.await(5,TimeUnit.SECONDS),"Media browser query timed out");
            if(id[0]!=null)return id[0];Thread.sleep(300);
        }
        throw new AssertionError("Imported fixture did not appear in the car library");
    }
    private void assertAdvances()throws Exception{
        awaitState(PlaybackState.STATE_PLAYING);long before=state().getPosition();Thread.sleep(2200);
        check(state().getState()==PlaybackState.STATE_PLAYING&&state().getPosition()>before,"Playback stopped advancing");
    }
    private void awaitMicrophone(boolean active)throws Exception{
        AudioManager audio=(AudioManager)getTargetContext().getSystemService(Context.AUDIO_SERVICE);
        long deadline=SystemClock.elapsedRealtime()+15000;
        while(SystemClock.elapsedRealtime()<deadline){if((!audio.getActiveRecordingConfigurations().isEmpty())==active)return;Thread.sleep(200);}
        throw new AssertionError("Microphone activity did not become "+active);
    }
    private void notificationAction(String title)throws Exception{
        NotificationManager notifications=(NotificationManager)getTargetContext().getSystemService(Context.NOTIFICATION_SERVICE);
        long deadline=SystemClock.elapsedRealtime()+15000;
        while(SystemClock.elapsedRealtime()<deadline){
            for(android.service.notification.StatusBarNotification posted:notifications.getActiveNotifications()){
                Notification.Action[] actions=posted.getNotification().actions;if(actions==null)continue;
                for(Notification.Action action:actions)if(title.contentEquals(action.title)&&action.actionIntent!=null){action.actionIntent.send();return;}
            }
            Thread.sleep(200);
        }
        throw new AssertionError("Recording notification action was not available: "+title);
    }
    private void verifyRecording()throws Exception{
        File directory=new File(getTargetContext().getFilesDir(),"recordings");
        HashSet<String> previous=new HashSet<>();File[] before=directory.listFiles();
        if(before!=null)for(File file:before)previous.add(file.getName());
        nativeCommand("record","",0);awaitMicrophone(true);Thread.sleep(1000);
        notificationAction("Pause recording");awaitMicrophone(false);Thread.sleep(500);
        notificationAction("Resume recording");awaitMicrophone(true);Thread.sleep(1000);
        notificationAction("Stop recording");awaitMicrophone(false);Thread.sleep(500);
        long deadline=SystemClock.elapsedRealtime()+15000;File saved=null;
        while(SystemClock.elapsedRealtime()<deadline&&saved==null){
            File[] files=directory.listFiles((dir,name)->!previous.contains(name)&&name.startsWith("Voice ")&&name.endsWith(".wav"));
            if(files!=null)for(File file:files)if(file.length()>44){saved=file;break;}
            if(saved==null)Thread.sleep(200);
        }
        check(saved!=null,"No new finalized voice PCM was saved");
        try(RandomAccessFile wav=new RandomAccessFile(saved,"r")){
            byte[] header=new byte[44];wav.readFully(header);
            check(new String(header,0,4,"US-ASCII").equals("RIFF")&&new String(header,8,4,"US-ASCII").equals("WAVE"),"Saved capture is not a WAV file");
        }
        step("Notification actions paused, released, reopened and finalized a new microphone WAV");
    }
    private void clearSystemDialog()throws Exception{
        AccessibilityNodeInfo root=getUiAutomation().getRootInActiveWindow();if(root==null)return;
        try{
            if(!root.findAccessibilityNodeInfosByText("System UI").isEmpty()&&!root.findAccessibilityNodeInfosByText("responding").isEmpty()){
                systemUiDialogSeen=true;
                for(AccessibilityNodeInfo button:root.findAccessibilityNodeInfosByText("Wait")){
                    try{button.performAction(AccessibilityNodeInfo.ACTION_CLICK);}finally{button.recycle();}
                }
                Thread.sleep(500);step("Observed and dismissed an emulator System UI unresponsive dialog");
            }
        }finally{root.recycle();}
    }
    private void verifyFocus()throws Exception{
        AudioManager audio=(AudioManager)getTargetContext().getSystemService(Context.AUDIO_SERVICE);
        AudioFocusRequest interrupt=new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
            .setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).build())
            .setOnAudioFocusChangeListener(change->{},new Handler(Looper.getMainLooper())).build();
        try{
            check(audio.requestAudioFocus(interrupt)==AudioManager.AUDIOFOCUS_REQUEST_GRANTED,"Could not request transient audio focus");
            awaitState(PlaybackState.STATE_PAUSED);
            audio.abandonAudioFocusRequest(interrupt);assertAdvances();
            step("Android transient audio focus paused and resumed native playback");
            check(audio.requestAudioFocus(interrupt)==AudioManager.AUDIOFOCUS_REQUEST_GRANTED,"Could not repeat focus interruption");
            awaitState(PlaybackState.STATE_PAUSED);controller.getTransportControls().pause();Thread.sleep(1000);
            audio.abandonAudioFocusRequest(interrupt);Thread.sleep(2200);
            check(state().getState()==PlaybackState.STATE_PAUSED,"Focus gain undid an explicit user pause");
            controller.getTransportControls().play();assertAdvances();
            step("Explicit user pause prevented automatic audio-focus resume");
        }finally{audio.abandonAudioFocusRequest(interrupt);}
    }
    private void verifyAmbient()throws Exception{
        controller.getTransportControls().pause();awaitState(PlaybackState.STATE_PAUSED);
        long primaryPosition=state().getPosition();
        nativeCommand("noise-white","",0);awaitState(PlaybackState.STATE_PLAYING);
        AudioManager audio=(AudioManager)getTargetContext().getSystemService(Context.AUDIO_SERVICE);
        AudioFocusRequest interrupt=new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
            .setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).build())
            .setOnAudioFocusChangeListener(change->{},new Handler(Looper.getMainLooper())).build();
        try{
            check(audio.requestAudioFocus(interrupt)==AudioManager.AUDIOFOCUS_REQUEST_GRANTED,"Could not interrupt the ambient mix");
            awaitState(PlaybackState.STATE_PAUSED);audio.abandonAudioFocusRequest(interrupt);
            awaitState(PlaybackState.STATE_PLAYING);
            controller.getTransportControls().pause();awaitState(PlaybackState.STATE_PAUSED);
            controller.getTransportControls().play();awaitState(PlaybackState.STATE_PLAYING);
            nativeCommand("stop-ambient","",0);awaitState(PlaybackState.STATE_PAUSED);
            check(Math.abs(state().getPosition()-primaryPosition)<=250,"Ambient focus resume restarted a previously paused music track");
            controller.getTransportControls().play();assertAdvances();
            step("Ambient-only media controls and focus resume preserved the paused primary track");
        }finally{audio.abandonAudioFocusRequest(interrupt);nativeCommand("stop-ambient","",0);}
    }
    @Override public void onStart(){
        boolean success=false;String error="";
        try{
            command=Class.forName("org.auvemir.app.AuvemirActivity").getMethod("nativeCommand",String.class,String.class,long.class);
            // Instrumentation.onStart can race Application.onCreate. Connecting the
            // real browser service first establishes the process-lifetime engine.
            connect();nativeCommand("import",fixture().getAbsolutePath(),1);
            String id=importedTrack();step("Cold media-browser startup and real library import passed");
            controller.getTransportControls().playFromMediaId(id,null);assertAdvances();step("Headless native playback advanced");
            controller.getTransportControls().pause();awaitState(PlaybackState.STATE_PAUSED);long paused=state().getPosition();Thread.sleep(1500);
            check(Math.abs(state().getPosition()-paused)<=250,"Paused position advanced");
            controller.getTransportControls().play();assertAdvances();step("MediaSession pause and resume passed");
            verifyFocus();
            Intent launch=getTargetContext().getPackageManager().getLaunchIntentForPackage("org.auvemir.app");
            check(launch!=null,"Launch intent is missing");launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            Activity first=startActivitySync(launch);Thread.sleep(2000);assertAdvances();long beforeDestroy=state().getPosition();
            runOnMainSync(first::finish);Thread.sleep(2000);assertAdvances();step("Playback survived Activity destruction");
            Activity second=startActivitySync(launch);Thread.sleep(2000);assertAdvances();
            check(state().getPosition()>=beforeDestroy,"Activity replacement reset the existing playback position");
            step("A replacement Activity reattached without resetting playback");
            verifyRecording();
            verifyAmbient();
            clearSystemDialog();
            Bitmap screenshot=getUiAutomation().takeScreenshot();
            if(screenshot!=null){File destination=new File(getTargetContext().getFilesDir(),"smoke-ui.png");try(FileOutputStream out=new FileOutputStream(destination)){screenshot.compress(Bitmap.CompressFormat.PNG,100,out);}screenshot.recycle();step("Actual Android UI screenshot captured");}
            controller.getTransportControls().stop();runOnMainSync(second::finish);success=true;
        }catch(Throwable failure){error=failure.toString();}
        finally{if(browser!=null)runOnMainSync(()->browser.disconnect());}
        JSONObject report=new JSONObject();
        try{report.put("success",success);report.put("sdk",Build.VERSION.SDK_INT);report.put("steps",new JSONArray(steps));report.put("emulator_system_ui_dialog_observed",systemUiDialogSeen);report.put("error",error);}catch(JSONException ignored){}
        Bundle result=new Bundle();result.putString("report",report.toString());finish(success?Activity.RESULT_OK:Activity.RESULT_CANCELED,result);
    }
}
