use anyhow::{Context, Result, ensure};
use jni::{
    JNIEnv, JavaVM,
    objects::{GlobalRef, JClass, JObject, JString, JValue},
    sys::jlong,
};
use std::{path::PathBuf, sync::OnceLock};
struct ContextData {
    vm: JavaVM,
    activity: GlobalRef,
    path: PathBuf,
}
static CONTEXT: OnceLock<ContextData> = OnceLock::new();
fn call<T>(f: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>) -> Result<T>) -> Result<T> {
    let c = CONTEXT
        .get()
        .context("Android context is not initialized")?;
    let mut env = c.vm.attach_current_thread()?;
    let result = env.with_local_frame(32, |env| f(env, c.activity.as_obj()));
    if env.exception_check().unwrap_or(false) {
        env.exception_clear().ok();
        return Err(anyhow::anyhow!("Android operation failed"));
    }
    result
}
fn initialize(env: &mut JNIEnv<'_>, application: &JObject<'_>) -> Result<()> {
    let vm = env.get_java_vm()?;
    let path = env
        .call_method(application, "dataPath", "()Ljava/lang/String;", &[])?
        .l()?;
    let path: String = env.get_string(&JString::from(path))?.into();
    let activity = env.new_global_ref(application)?;
    let _ = CONTEXT.set(ContextData {
        vm,
        activity,
        path: PathBuf::from(path),
    });
    let context = CONTEXT
        .get()
        .context("Android context initialization failed")?;
    // The global Application reference remains owned by CONTEXT for the process lifetime.
    unsafe {
        ndk_context::set_application_context(
            context.vm.get_java_vm_pointer().cast(),
            context.activity.as_obj().as_raw().cast(),
        );
    }
    let vm22 = unsafe { jni22::JavaVM::from_raw(context.vm.get_java_vm_pointer().cast()) };
    vm22.attach_current_thread(|env22| {
        let object22 = unsafe {
            jni22::objects::JObject::from_raw(env22, context.activity.as_obj().as_raw().cast())
        };
        rustls_platform_verifier::android::init_with_env(env22, object22)
    })?;
    crate::app::start_background()
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_auvemir_app_AuvemirApplication_nativeStart(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    application: JObject<'_>,
) -> jni::sys::jboolean {
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("Auvemir")
            .with_max_level(log::LevelFilter::Info),
    );
    match initialize(&mut env, &application) {
        Ok(()) => 1,
        Err(error) => {
            log::error!("Audio initialization failed: {error:#}");
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
            }
            0
        }
    }
}
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    let result = (|| -> Result<()> {
        // Application.onCreate initializes the persistent worker before NativeActivity starts.
        if CONTEXT.get().is_none() {
            let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
            let mut env = vm.attach_current_thread()?;
            let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
            let application = env
                .call_method(
                    &activity,
                    "getApplicationContext",
                    "()Landroid/content/Context;",
                    &[],
                )?
                .l()?;
            initialize(&mut env, &application)?;
        }
        slint::android::init(app)?;
        crate::run()
    })();
    if let Err(error) = result {
        log::error!("Activity stopped: {error:#}");
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_auvemir_app_AuvemirActivity_nativeCommand(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    action: JString<'_>,
    value: JString<'_>,
    n: jlong,
) {
    let result = (|| -> Result<()> {
        let action: String = env.get_string(&action)?.into();
        let value: String = env.get_string(&value)?.into();
        ensure!(
            action.len() <= 128 && value.len() <= 1024 * 1024,
            "Native command is too large"
        );
        crate::platform::command(action, value, n);
        Ok(())
    })();
    if let Err(e) = result {
        log::warn!("Native command rejected: {e}");
    }
}
pub fn data_dir() -> Result<PathBuf> {
    Ok(CONTEXT
        .get()
        .context("Android is not initialized")?
        .path
        .clone())
}
pub fn pick(page: i32, folder: bool, action: &str) -> Result<()> {
    call(|env, activity| {
        let action = env.new_string(action)?;
        env.call_method(
            activity,
            "pick",
            "(IZLjava/lang/String;)V",
            &[
                JValue::Int(page),
                JValue::Bool(folder as u8),
                JValue::Object(action.as_ref()),
            ],
        )?;
        Ok(())
    })
}
pub fn microphone_ready() -> Result<bool> {
    call(|env, activity| {
        Ok(env
            .call_method(activity, "microphoneReady", "()Z", &[])?
            .z()?)
    })
}
pub fn activate_audio(recording: bool) -> Result<()> {
    call(|env, activity| {
        env.call_method(
            activity,
            "activateAudio",
            "(Z)V",
            &[JValue::Bool(recording as u8)],
        )?;
        Ok(())
    })
}
pub fn recording(state: i32) {
    let _ = call(|env, activity| {
        env.call_method(activity, "recording", "(I)V", &[JValue::Int(state)])?;
        Ok(())
    });
}
pub fn now_playing(
    media: Option<&auvemir_core::model::Media>,
    playing: bool,
    position: u64,
    _volume: f32,
    speed: f32,
    can_seek: bool,
) {
    let _ = call(|env, activity| {
        let title = env.new_string(media.map(|m| m.title.as_str()).unwrap_or(""))?;
        let artist = env.new_string(media.map(|m| m.artist.as_str()).unwrap_or(""))?;
        let id = env.new_string(media.map(|m| m.id.as_str()).unwrap_or(""))?;
        env.call_method(
            activity,
            "updatePlayback",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;JJZFZ)V",
            &[
                JValue::Object(id.as_ref()),
                JValue::Object(title.as_ref()),
                JValue::Object(artist.as_ref()),
                JValue::Long(
                    media
                        .map(|m| m.duration_ms)
                        .unwrap_or(0)
                        .min(i64::MAX as u64) as i64,
                ),
                JValue::Long(position.min(i64::MAX as u64) as i64),
                JValue::Bool(playing as u8),
                JValue::Float(speed),
                JValue::Bool(can_seek as u8),
            ],
        )?;
        Ok(())
    });
}
pub fn credential(action: &str, id: &str, secret: &str) -> Result<String> {
    call(|env, activity| {
        let action = env.new_string(action)?;
        let id = env.new_string(id)?;
        let secret = env.new_string(secret)?;
        let result = env
            .call_method(
                activity,
                "credential",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                &[
                    JValue::Object(action.as_ref()),
                    JValue::Object(id.as_ref()),
                    JValue::Object(secret.as_ref()),
                ],
            )?
            .l()?;
        ensure!(
            !result.is_null(),
            "Secure credential storage is unavailable"
        );
        Ok(env.get_string(&JString::from(result))?.into())
    })
}
pub fn shutdown() {
    let _ = call(|env, activity| {
        env.call_method(activity, "stopAudio", "()V", &[])?;
        Ok(())
    });
}

pub fn export_document(path: &str, name: &str) -> Result<()> {
    call(|env, activity| {
        let path = env.new_string(path)?;
        let name = env.new_string(name)?;
        env.call_method(
            activity,
            "exportDocument",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[JValue::Object(path.as_ref()), JValue::Object(name.as_ref())],
        )?;
        Ok(())
    })
}

pub fn user_transport() {
    let _ = call(|env, application| {
        env.call_method(application, "userTransport", "()V", &[])?;
        Ok(())
    });
}

pub fn reduced_motion() -> Result<bool> {
    call(|env, app| Ok(env.call_method(app, "reducedMotion", "()Z", &[])?.z()?))
}
