package org.auvemir.app;

import android.Manifest;
import android.app.Application;
import java.lang.ref.WeakReference;
import android.content.*;
import android.content.pm.PackageManager;
import android.media.AudioManager;
import android.net.Uri;
import android.os.*;
import android.provider.DocumentsContract;
import android.database.Cursor;
import android.security.keystore.*;
import android.util.Base64;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.security.*;
import java.util.*;
import java.util.concurrent.*;
import javax.crypto.*;
import javax.crypto.spec.GCMParameterSpec;

/** Process-lifetime platform bridge; foreground audio does not retain an Activity. */
public final class AuvemirApplication extends Application {
    static { System.loadLibrary("auvemir"); }
    private static native boolean nativeStart(AuvemirApplication app);
    private volatile WeakReference<AuvemirActivity> current = new WeakReference<>(null);
    @Override public void onCreate(){super.onCreate();ensureEngine();}
    public boolean ensureEngine(){return nativeStart(this);}
    public void attach(AuvemirActivity activity){current=new WeakReference<>(activity);}
    public void detach(AuvemirActivity activity){if(current.get()==activity)current.clear();}
    private AuvemirActivity activity(){AuvemirActivity result=current.get();if(result==null)throw new IllegalStateException("Open Auvemir on your phone for this action");return result;}
    public String dataPath(){return getFilesDir().getAbsolutePath();}
    public void pick(int page,boolean folder,String action){activity().pick(page,folder,action);}
    public void exportDocument(String source,String name){activity().exportDocument(source,name);}
    public boolean microphoneReady(){if(checkSelfPermission(Manifest.permission.RECORD_AUDIO)==PackageManager.PERMISSION_GRANTED)return true;AuvemirActivity active=current.get();if(active!=null)return active.microphoneReady();return false;}
    public void activateAudio(boolean recording){AuvemirActivity active=current.get();if(active!=null&&Build.VERSION.SDK_INT>=33&&checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)!=PackageManager.PERMISSION_GRANTED)active.runOnUiThread(()->active.requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS},201));AudioService.activate(this,recording);}
    public void updatePlayback(String id,String title,String artist,long duration,long position,boolean playing,float speed,boolean canSeek){AudioService.update(this,id,title,artist,duration,position,playing,speed,canSeek);}
    public void recording(int state){AudioService.setRecording(state);}
    public void userTransport(){AudioService.userTransport();}
    public void stopAudio(){stopService(new Intent(this,AudioService.class));}
    public synchronized String credential(String action,String id,String secret)throws Exception{
        if(id.length()>1024||secret.length()>65536)throw new IOException("Credential is too large");
        SharedPreferences prefs=getSharedPreferences("secure-credentials",MODE_PRIVATE);if(action.equals("remove")){if(!prefs.edit().remove(id).commit())throw new IOException("Cannot remove credential");return "";}
        KeyStore store=KeyStore.getInstance("AndroidKeyStore");store.load(null);String alias="Auvemir.credentials";
        if(!store.containsAlias(alias)){KeyGenerator generator=KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES,"AndroidKeyStore");generator.init(new KeyGenParameterSpec.Builder(alias,KeyProperties.PURPOSE_ENCRYPT|KeyProperties.PURPOSE_DECRYPT).setKeySize(256).setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build());generator.generateKey();}
        SecretKey key=(SecretKey)store.getKey(alias,null);Cipher cipher=Cipher.getInstance("AES/GCM/NoPadding");
        if(action.equals("save")){cipher.init(Cipher.ENCRYPT_MODE,key);cipher.updateAAD(id.getBytes(StandardCharsets.UTF_8));byte[] iv=cipher.getIV(),encrypted=cipher.doFinal(secret.getBytes(StandardCharsets.UTF_8));byte[] data=new byte[iv.length+encrypted.length];System.arraycopy(iv,0,data,0,iv.length);System.arraycopy(encrypted,0,data,iv.length,encrypted.length);if(!prefs.edit().putString(id,Base64.encodeToString(data,Base64.NO_WRAP)).commit())throw new IOException("Cannot save credential");return "";}
        if(!action.equals("load"))throw new IOException("Unknown credential action");String encoded=prefs.getString(id,null);if(encoded==null)throw new IOException("Reconnect this service");byte[] data=Base64.decode(encoded,Base64.NO_WRAP);if(data.length<28)throw new IOException("Invalid credential");cipher.init(Cipher.DECRYPT_MODE,key,new GCMParameterSpec(128,data,0,12));cipher.updateAAD(id.getBytes(StandardCharsets.UTF_8));return new String(cipher.doFinal(data,12,data.length-12),StandardCharsets.UTF_8);
    }
}
