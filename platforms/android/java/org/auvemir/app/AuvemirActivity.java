package org.auvemir.app;

import android.Manifest;
import android.app.NativeActivity;
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

public final class AuvemirActivity extends NativeActivity {
    static { System.loadLibrary("auvemir"); }
    public static native void nativeCommand(String action,String value,long number);
    static void command(String action) { nativeCommand(action,"",0); }
    private final ThreadPoolExecutor imports=new ThreadPoolExecutor(1,1,30,TimeUnit.SECONDS,new ArrayBlockingQueue<Runnable>(8),new ThreadPoolExecutor.AbortPolicy());
    private File exportSource;
    private int importPage=1; private String fileAction="";
    public String dataPath(){return getFilesDir().getAbsolutePath();}
    public void pick(final int page,final boolean folder,final String action){runOnUiThread(()->{
        importPage=page;fileAction=action;
        Intent intent=new Intent(folder?Intent.ACTION_OPEN_DOCUMENT_TREE:Intent.ACTION_OPEN_DOCUMENT);
        if(!folder){intent.setType(action.isEmpty()?"audio/*":"*/*");intent.addCategory(Intent.CATEGORY_OPENABLE);intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE,action.isEmpty());}
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
        startActivityForResult(intent,folder?101:100);
    });}
    @Override protected void onActivityResult(int request,int result,Intent data){
        super.onActivityResult(request,result,data);
        if(request==102){final File source=exportSource;exportSource=null;if(source==null)return;if(result!=RESULT_OK||data==null||data.getData()==null){cleanupExport(source);return;}final Uri destination=data.getData();
            try{imports.execute(()->{try(InputStream input=new FileInputStream(source);OutputStream output=getContentResolver().openOutputStream(destination,"w")){if(output==null)throw new IOException("Cannot write selected destination");byte[] buffer=new byte[65536];int n;while((n=input.read(buffer))!=-1)output.write(buffer,0,n);output.flush();nativeCommand("notice","Export saved.",0);}catch(Exception e){nativeCommand("notice","Export failed: "+e.getMessage(),0);}finally{cleanupExport(source);}});}catch(RejectedExecutionException e){cleanupExport(source);nativeCommand("notice","Export queue is busy. Try again shortly.",0);}return;}
        if((request!=100&&request!=101)||result!=RESULT_OK||data==null)return;
        final int page=importPage;final String action=fileAction;final ArrayList<Uri> uris=new ArrayList<>();
        if(data.getClipData()!=null){for(int i=0;i<data.getClipData().getItemCount()&&i<50000;i++)uris.add(data.getClipData().getItemAt(i).getUri());}else if(data.getData()!=null)uris.add(data.getData());
        for(Uri uri:uris){try{getContentResolver().takePersistableUriPermission(uri,Intent.FLAG_GRANT_READ_URI_PERMISSION);}catch(SecurityException ignored){}}
        try{imports.execute(()->{try{
            File destination=new File(getFilesDir(),action.isEmpty()?"imports":"documents");if(!destination.isDirectory()&&!destination.mkdirs())throw new IOException("Cannot create import folder");
            int[] count={0};for(Uri uri:uris){if(request==101){String root=DocumentsContract.getTreeDocumentId(uri);copyTree(uri,root,destination,0,count);}else {File file=copy(uri,destination,action.isEmpty());if(!action.isEmpty())nativeCommand("file:"+action,file.getAbsolutePath(),page);}}
            if(action.isEmpty())nativeCommand("import",destination.getAbsolutePath(),page);
        }catch(Exception e){nativeCommand("notice","Import failed: "+e.getMessage(),0);}});}catch(RejectedExecutionException e){nativeCommand("notice","An import is already queued. Try again shortly.",0);}
    }
    private void copyTree(Uri tree,String parent,File destination,int depth,int[] count)throws Exception{
        if(depth>32)throw new IOException("Folder nesting exceeds 32 levels");
        Uri children=DocumentsContract.buildChildDocumentsUriUsingTree(tree,parent);
        try(Cursor c=getContentResolver().query(children,new String[]{DocumentsContract.Document.COLUMN_DOCUMENT_ID,DocumentsContract.Document.COLUMN_MIME_TYPE},null,null,null)){
            if(c==null)return;while(c.moveToNext()){
                if(++count[0]>50000)throw new IOException("Import exceeds 50,000 entries");String id=c.getString(0);
                if(DocumentsContract.Document.MIME_TYPE_DIR.equals(c.getString(1)))copyTree(tree,id,destination,depth+1,count);else copy(DocumentsContract.buildDocumentUriUsingTree(tree,id),destination,true);
            }
        }
    }
    private File copy(Uri uri,File destination,boolean audioOnly)throws Exception{
        String name="audio";try(Cursor c=getContentResolver().query(uri,new String[]{DocumentsContract.Document.COLUMN_DISPLAY_NAME},null,null,null)){if(c!=null&&c.moveToFirst())name=c.getString(0);}
        name=name==null?"audio":name.replaceAll("[^\\p{L}\\p{N}._ -]","_");
        if(audioOnly&&!name.toLowerCase(Locale.ROOT).matches(".*\\.(mp3|flac|wav|ogg|opus|aac|m4a|m4b|alac|aiff|aif|mp2|wma|ape|wv|dsf|caf)$"))return new File(destination,"skipped");
        MessageDigest digest=MessageDigest.getInstance("SHA-256");byte[] hash=digest.digest(uri.toString().getBytes(StandardCharsets.UTF_8));StringBuilder prefix=new StringBuilder();for(int i=0;i<12;i++)prefix.append(String.format(Locale.ROOT,"%02x",hash[i]));
        File file=new File(destination,prefix+"-"+name);File temp=new File(destination,file.getName()+".partial");
        try(InputStream input=getContentResolver().openInputStream(uri);FileOutputStream output=new FileOutputStream(temp)){
            if(input==null)throw new IOException("Cannot read selected document");byte[] buffer=new byte[65536];long total=0;int n;
            while((n=input.read(buffer))!=-1){total+=n;if(total>2L*1024*1024*1024)throw new IOException("File exceeds 2 GiB");if(destination.getUsableSpace()<16L*1024*1024+n)throw new IOException("Insufficient free storage");output.write(buffer,0,n);}output.getFD().sync();
        }catch(Exception e){temp.delete();throw e;}
        if(!temp.renameTo(file)){temp.delete();throw new IOException("Cannot finish importing file");}return file;
    }
    public void exportDocument(String source,String name){runOnUiThread(()->{exportSource=new File(source);Intent intent=new Intent(Intent.ACTION_CREATE_DOCUMENT).setType("application/octet-stream").addCategory(Intent.CATEGORY_OPENABLE).putExtra(Intent.EXTRA_TITLE,name);startActivityForResult(intent,102);});}
    private void cleanupExport(File source){try{if(source.getCanonicalPath().startsWith(new File(getFilesDir(),"exports").getCanonicalPath()+File.separator))source.delete();}catch(IOException ignored){}}
    public boolean microphoneReady(){if(checkSelfPermission(Manifest.permission.RECORD_AUDIO)==PackageManager.PERMISSION_GRANTED)return true;runOnUiThread(()->requestPermissions(new String[]{Manifest.permission.RECORD_AUDIO},200));return false;}
    @Override public void onRequestPermissionsResult(int request,String[] permissions,int[] results){super.onRequestPermissionsResult(request,permissions,results);if(request==200&&results.length>0&&results[0]==PackageManager.PERMISSION_GRANTED)command("record");}
    public void activateAudio(boolean recording){
        if(Build.VERSION.SDK_INT>=33&&checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)!=PackageManager.PERMISSION_GRANTED)runOnUiThread(()->requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS},201));
        AudioService.activate(this,recording);
    }
    public void updatePlayback(String id,String title,String artist,long duration,long position,boolean playing,float speed,boolean canSeek){AudioService.update(this,id,title,artist,duration,position,playing,speed,canSeek);}
    public void recording(int state){AudioService.setRecording(state);}
    public void stopAudio(){stopService(new Intent(this,AudioService.class));}
    public String credential(String action,String id,String secret)throws Exception{return ((AuvemirApplication)getApplication()).credential(action,id,secret);}
    @Override protected void onCreate(Bundle state){imports.allowCoreThreadTimeOut(true);((AuvemirApplication)getApplication()).attach(this);super.onCreate(state);}
    @Override protected void onDestroy(){((AuvemirApplication)getApplication()).detach(this);imports.shutdown();super.onDestroy();}
}
