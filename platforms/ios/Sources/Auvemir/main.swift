import UIKit
import AVFoundation
import AVKit
import MediaPlayer
import Security
import UniformTypeIdentifiers

@_silgen_name("auvemir_ios_run") func rustRun() -> Int32
@_silgen_name("auvemir_ios_command") func rustCommand(_ action:UnsafePointer<CChar>,_ value:UnsafePointer<CChar>,_ number:Int64)
func command(_ action:String,_ value:String="",_ n:Int64=0){ action.withCString{a in value.withCString{v in rustCommand(a,v,n)}} }
func dataDirectory() -> URL { FileManager.default.urls(for:.applicationSupportDirectory,in:.userDomainMask)[0].appendingPathComponent("Auvemir",isDirectory:true) }
func topController()->UIViewController? { let root=UIApplication.shared.connectedScenes.compactMap{$0 as? UIWindowScene}.flatMap{$0.windows}.first{$0.isKeyWindow}?.rootViewController;var top=root;while let shown=top?.presentedViewController{top=shown};return top }
final class Host:NSObject,UIDocumentPickerDelegate {
    static let shared=Host();var page:Int32=1;var fileAction="";var exportSource:URL?;var capturing=false;var installed=false;var resume=false;var lastSecond:Int64 = -1;var lastMetadata="";var lastPlaying=false
    let imports=DispatchQueue(label:"org.auvemir.imports",qos:.utility)
    func install(){if installed{return};installed=true
        let remote=MPRemoteCommandCenter.shared()
        remote.playCommand.addTarget{_ in command("play");return .success};remote.pauseCommand.addTarget{_ in command("pause");return .success};remote.togglePlayPauseCommand.addTarget{_ in command("toggle");return .success};remote.stopCommand.addTarget{_ in command("stop");return .success};remote.nextTrackCommand.addTarget{_ in command("next");return .success};remote.previousTrackCommand.addTarget{_ in command("previous");return .success}
        remote.changePlaybackPositionCommand.addTarget{event in guard let event=event as? MPChangePlaybackPositionCommandEvent else{return .commandFailed};command("seek","",Int64(event.positionTime*1000));return .success}
        NotificationCenter.default.addObserver(forName:AVAudioSession.interruptionNotification,object:nil,queue:.main){note in
            guard let type=note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt else{return}
            if type==AVAudioSession.InterruptionType.began.rawValue{self.resume=self.resume || self.lastPlaying;command("focus-pause");if self.capturing{command("pause-recording")}}
            else{let shouldResume=self.resume && (note.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0)&AVAudioSession.InterruptionOptions.shouldResume.rawValue != 0;self.resume=false;if shouldResume{command("focus-play")}}
        }
        NotificationCenter.default.addObserver(forName:AVAudioSession.routeChangeNotification,object:nil,queue:.main){note in if (note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt)==AVAudioSession.RouteChangeReason.oldDeviceUnavailable.rawValue{command("pause")} }
    }
    func documentPicker(_ controller:UIDocumentPickerViewController,didPickDocumentsAt urls:[URL]){
        if let source=exportSource {exportSource=nil;cleanupExport(source);command("notice","Export saved.");return}
        let page=self.page,action=self.fileAction
        imports.async{do{
            let destination=dataDirectory().appendingPathComponent(action.isEmpty ? "imports":"documents",isDirectory:true);try FileManager.default.createDirectory(at:destination,withIntermediateDirectories:true)
            var count=0
            for url in urls{let access=url.startAccessingSecurityScopedResource();defer{if access{url.stopAccessingSecurityScopedResource()}}
                try self.copyTree(url,destination,0,&count,audioOnly:action.isEmpty)
            }
            if action.isEmpty{command("import",destination.path,Int64(page))}else if let url=urls.first{command("file:"+action,destination.appendingPathComponent(url.lastPathComponent).path,Int64(page))}
        }catch{command("notice","Import failed: \(error.localizedDescription)")}}
    }
    func documentPickerWasCancelled(_ controller:UIDocumentPickerViewController){if let source=exportSource{exportSource=nil;cleanupExport(source)}}
    func cleanupExport(_ source:URL){if source.standardizedFileURL.path.hasPrefix(dataDirectory().appendingPathComponent("exports").standardizedFileURL.path+"/"){try? FileManager.default.removeItem(at:source)}}
    func copyTree(_ source:URL,_ destination:URL,_ depth:Int,_ count:inout Int,audioOnly:Bool)throws{
        guard depth<=32,count<50000 else{throw NSError(domain:"Auvemir",code:1,userInfo:[NSLocalizedDescriptionKey:"Import exceeds folder limits"])};count+=1
        let properties=try source.resourceValues(forKeys:[.isDirectoryKey,.isSymbolicLinkKey,.fileSizeKey]);if properties.isSymbolicLink==true{return}
        if properties.isDirectory==true{
            let child=destination.appendingPathComponent(source.lastPathComponent,isDirectory:true);try FileManager.default.createDirectory(at:child,withIntermediateDirectories:true)
            for url in try FileManager.default.contentsOfDirectory(at:source,includingPropertiesForKeys:nil){try copyTree(url,child,depth+1,&count,audioOnly:audioOnly)};return
        }
        if audioOnly && !["mp3","flac","wav","ogg","opus","aac","m4a","m4b","aiff","aif","mp2","alac","wma","ape","wv","dsf","caf"].contains(source.pathExtension.lowercased()){return}
        let size=Int64(properties.fileSize ?? 0);let free=(try FileManager.default.attributesOfFileSystem(forPath:destination.path)[.systemFreeSize] as? NSNumber)?.int64Value ?? 0
        guard size<=2*1024*1024*1024,free>=size+16*1024*1024 else{throw NSError(domain:"Auvemir",code:2,userInfo:[NSLocalizedDescriptionKey:"Not enough storage, or file exceeds 2 GiB"])}
        let target=destination.appendingPathComponent(source.lastPathComponent);let temp=destination.appendingPathComponent(UUID().uuidString+".partial")
        do{try FileManager.default.copyItem(at:source,to:temp);if FileManager.default.fileExists(atPath:target.path){_ = try FileManager.default.replaceItemAt(target,withItemAt:temp)}else{try FileManager.default.moveItem(at:temp,to:target)}}catch{try? FileManager.default.removeItem(at:temp);throw error}
    }
}
@_cdecl("auvemir_data_path") func nativeData()->UnsafeMutablePointer<CChar>? { let path=dataDirectory();do{try FileManager.default.createDirectory(at:path,withIntermediateDirectories:true);return strdup(path.path)}catch{return nil} }
@_cdecl("auvemir_pick") func nativePick(_ page:Int32,_ folder:Int32,_ action:UnsafePointer<CChar>){let text=String(cString:action);DispatchQueue.main.async{let host=Host.shared;host.page=page;host.fileAction=text;let picker=UIDocumentPickerViewController(forOpeningContentTypes:folder != 0 ? [.folder] : text.isEmpty ? [.audio]:[.data],asCopy:false);picker.allowsMultipleSelection=text.isEmpty;picker.delegate=host;topController()?.present(picker,animated:true)} }
@_cdecl("auvemir_export") func nativeExport(_ source:UnsafePointer<CChar>,_ name:UnsafePointer<CChar>){let url=URL(fileURLWithPath:String(cString:source));DispatchQueue.main.async{let host=Host.shared;host.exportSource=url;let picker=UIDocumentPickerViewController(forExporting:[url],asCopy:true);picker.delegate=host;topController()?.present(picker,animated:true)}}
@_cdecl("auvemir_microphone") func nativeMicrophone()->Int32{let session=AVAudioSession.sharedInstance();switch session.recordPermission{case .granted:return 1;case .denied:command("notice","Enable microphone access for Auvemir in Settings.");return 0;default:session.requestRecordPermission{granted in if granted{command("record")}else{command("notice","Microphone access was declined.")}};return 0} }
@_cdecl("auvemir_activate") func nativeActivate(_ recording:Int32)->Int32{do{let session=AVAudioSession.sharedInstance();try session.setCategory(recording != 0 ? .playAndRecord:.playback,mode:.default,options:recording != 0 ? [.defaultToSpeaker,.allowBluetooth,.allowAirPlay]:[.allowAirPlay]);try session.setActive(true);DispatchQueue.main.async{Host.shared.install()};return 1}catch{return 0} }
@_cdecl("auvemir_recording") func nativeRecording(_ active:Int32){DispatchQueue.main.async{Host.shared.capturing=active == 1}}
@_cdecl("auvemir_now_playing") func nativeNow(_ text:UnsafePointer<CChar>) {
    let data=Data(String(cString:text).utf8)
    guard let json=(try? JSONSerialization.jsonObject(with:data)) as? [String:Any] else{return}
    DispatchQueue.main.async {
        let host=Host.shared
        let p=(json["position"] as? NSNumber)?.int64Value ?? 0
        let playing=json["playing"] as? Bool ?? false
        let speed=(json["speed"] as? NSNumber)?.doubleValue ?? 1
        let seekable=json["canSeek"] as? Bool ?? false
        let metadata=[String(describing:json["id"] ?? ""),String(describing:json["title"] ?? ""),String(describing:json["artist"] ?? ""),String(describing:json["duration"] ?? 0),String(speed),String(seekable)].joined(separator:"\u{0}")
        if p/1000==host.lastSecond && playing==host.lastPlaying && metadata==host.lastMetadata{return}
        host.lastSecond=p/1000;host.lastPlaying=playing;host.lastMetadata=metadata
        MPRemoteCommandCenter.shared().changePlaybackPositionCommand.isEnabled=seekable
        MPNowPlayingInfoCenter.default().nowPlayingInfo=[
            MPMediaItemPropertyTitle:json["title"] ?? "",
            MPMediaItemPropertyArtist:json["artist"] ?? "",
            MPMediaItemPropertyPlaybackDuration:((json["duration"] as? NSNumber)?.doubleValue ?? 0)/1000,
            MPNowPlayingInfoPropertyElapsedPlaybackTime:Double(p)/1000,
            MPNowPlayingInfoPropertyPlaybackRate:playing ? speed:0
        ]
    }
}
@_cdecl("auvemir_credential") func nativeCredential(_ action:UnsafePointer<CChar>,_ identifier:UnsafePointer<CChar>,_ secret:UnsafePointer<CChar>)->UnsafeMutablePointer<CChar>? {
    let action=String(cString:action),id=String(cString:identifier),secret=String(cString:secret)
    let query:[String:Any]=[kSecClass as String:kSecClassGenericPassword,kSecAttrService as String:"Auvemir",kSecAttrAccount as String:id]
    if action=="remove"{let status=SecItemDelete(query as CFDictionary);return status==errSecSuccess || status==errSecItemNotFound ? strdup(""):nil}
    if action=="save"{let changes:[String:Any]=[kSecValueData as String:Data(secret.utf8),kSecAttrAccessible as String:kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly];let status=SecItemUpdate(query as CFDictionary,changes as CFDictionary);if status==errSecSuccess{return strdup("")};if status != errSecItemNotFound{return nil};var added=query;for(k,v)in changes{added[k]=v};return SecItemAdd(added as CFDictionary,nil)==errSecSuccess ? strdup(""):nil}
    guard action=="load" else{return nil};var read=query;read[kSecReturnData as String]=true;read[kSecMatchLimit as String]=kSecMatchLimitOne;var result:CFTypeRef?;guard SecItemCopyMatching(read as CFDictionary,&result)==errSecSuccess,let data=result as? Data,let text=String(data:data,encoding:.utf8)else{return nil};return strdup(text)
}
@_cdecl("auvemir_user_transport") func nativeUserTransport(){DispatchQueue.main.async{Host.shared.resume=false}}
@_cdecl("auvemir_shutdown") func nativeShutdown(){try? AVAudioSession.sharedInstance().setActive(false,options:.notifyOthersOnDeactivation);DispatchQueue.main.async{MPNowPlayingInfoCenter.default().nowPlayingInfo=nil}}
@_cdecl("auvemir_routes") func nativeRoutes(){DispatchQueue.main.async{let controller=UIViewController();controller.view.backgroundColor = .black;let picker=AVRoutePickerView();picker.tintColor = .white;picker.translatesAutoresizingMaskIntoConstraints=false;controller.view.addSubview(picker);NSLayoutConstraint.activate([picker.centerXAnchor.constraint(equalTo:controller.view.centerXAnchor),picker.centerYAnchor.constraint(equalTo:controller.view.centerYAnchor),picker.widthAnchor.constraint(equalToConstant:80),picker.heightAnchor.constraint(equalToConstant:80)]);controller.modalPresentationStyle = .pageSheet;topController()?.present(controller,animated:true)}}
exit(rustRun())

@_cdecl("auvemir_reduce_motion") func nativeReduceMotion() -> Bool { UIAccessibility.isReduceMotionEnabled }
