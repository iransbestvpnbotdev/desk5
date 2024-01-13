use std::{
    collections::HashMap,
    iter::FromIterator,
    sync::{Arc, Mutex},
};

use sciter::Value;

use hbb_common::{
    allow_err,
    config::{LocalConfig, PeerConfig},
    log,
};

#[cfg(not(any(feature = "flutter", feature = "cli")))]
use crate::ui_session_interface::Session;
use crate::{common::get_app_name, ipc, ui_interface::*};

mod cm;
#[cfg(feature = "inline")]
pub mod inline;
pub mod remote;

#[allow(dead_code)]
type Status = (i32, bool, i64, String);

lazy_static::lazy_static! {
    // stupid workaround for https://sciter.com/forums/topic/crash-on-latest-tis-mac-sdk-sometimes/
    static ref STUPID_VALUES: Mutex<Vec<Arc<Vec<Value>>>> = Default::default();
}

#[cfg(not(any(feature = "flutter", feature = "cli")))]
lazy_static::lazy_static! {
    pub static ref CUR_SESSION: Arc<Mutex<Option<Session<remote::SciterHandler>>>> = Default::default();
}

struct UIHostHandler;

pub fn start(args: &mut [String]) {
    #[cfg(target_os = "macos")]
    crate::platform::delegate::show_dock();
    #[cfg(all(target_os = "linux", feature = "inline"))]
    {
        #[cfg(feature = "appimage")]
        let prefix = std::env::var("APPDIR").unwrap_or("".to_string());
        #[cfg(not(feature = "appimage"))]
        let prefix = "".to_string();
        #[cfg(feature = "flatpak")]
        let dir = "/app";
        #[cfg(not(feature = "flatpak"))]
        let dir = "/usr";
        sciter::set_library(&(prefix + dir + "/lib/remotend/libsciter-gtk.so")).ok();
    }
    #[cfg(windows)]
    // Check if there is a sciter.dll nearby.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let sciter_dll_path = parent.join("sciter.dll");
            if sciter_dll_path.exists() {
                // Try to set the sciter dll.
                let p = sciter_dll_path.to_string_lossy().to_string();
                log::debug!("Found dll:{}, \n {:?}", p, sciter::set_library(&p));
            }
        }
    }
    // https://github.com/c-smile/sciter-sdk/blob/master/include/sciter-x-types.h
    // https://github.com/issues/132#issuecomment-886069737
    #[cfg(windows)]
    allow_err!(sciter::set_options(sciter::RuntimeOptions::GfxLayer(
        sciter::GFX_LAYER::WARP
    )));
    use sciter::SCRIPT_RUNTIME_FEATURES::*;
    allow_err!(sciter::set_options(sciter::RuntimeOptions::ScriptFeatures(
        ALLOW_FILE_IO as u8 | ALLOW_SOCKET_IO as u8 | ALLOW_EVAL as u8 | ALLOW_SYSINFO as u8
    )));
    let mut frame = sciter::WindowBuilder::main_window().create();
    #[cfg(windows)]
    allow_err!(sciter::set_options(sciter::RuntimeOptions::UxTheming(true)));
    frame.set_title(&crate::get_app_name());
    #[cfg(target_os = "macos")]
    crate::platform::delegate::make_menubar(frame.get_host(), args.is_empty());
    let page;
    if args.len() > 1 && args[0] == "--play" {
        args[0] = "--connect".to_owned();
        let path: std::path::PathBuf = (&args[1]).into();
        let id = path
            .file_stem()
            .map(|p| p.to_str().unwrap_or(""))
            .unwrap_or("")
            .to_owned();
        args[1] = id;
    }
    if args.is_empty() {
        std::thread::spawn(move || check_zombie());
        crate::common::check_software_update();
        frame.event_handler(UI {});
        frame.sciter_handler(UIHostHandler {});
        page = "index.html";
        // Start pulse audio local server.
        #[cfg(target_os = "linux")]
        std::thread::spawn(crate::ipc::start_pa);
    } else if args[0] == "--install" {
        frame.event_handler(UI {});
        frame.sciter_handler(UIHostHandler {});
        page = "install.html";
    } else if args[0] == "--cm" {
        frame.register_behavior("connection-manager", move || {
            Box::new(cm::SciterConnectionManager::new())
        });
        page = "cm.html";
    } else if (args[0] == "--connect"
        || args[0] == "--file-transfer"
        || args[0] == "--port-forward"
        || args[0] == "--rdp")
        && args.len() > 1
    {
        #[cfg(windows)]
        {
            let hw = frame.get_host().get_hwnd();
            crate::platform::windows::enable_lowlevel_keyboard(hw as _);
        }
        let mut iter = args.iter();
        let Some(cmd) = iter.next() else {
            log::error!("Failed to get cmd arg");
            return;
        };
        let cmd = cmd.to_owned();
        let Some(id) = iter.next() else {
            log::error!("Failed to get id arg");
            return;
        };
        let id = id.to_owned();
        let pass = iter.next().unwrap_or(&"".to_owned()).clone();
        let args: Vec<String> = iter.map(|x| x.clone()).collect();
        frame.set_title(&id);
        frame.register_behavior("native-remote", move || {
            let handler =
                remote::SciterSession::new(cmd.clone(), id.clone(), pass.clone(), args.clone());
            #[cfg(not(any(feature = "flutter", feature = "cli")))]
            {
                *CUR_SESSION.lock().unwrap() = Some(handler.inner());
            }
            Box::new(handler)
        });
        page = "remote.html";
    } else {
        log::error!("Wrong command: {:?}", args);
        return;
    }
    #[cfg(feature = "inline")]
    {
        let html = if page == "index.html" {
            inline::get_index()
        } else if page == "cm.html" {
            inline::get_cm()
        } else if page == "install.html" {
            inline::get_install()
        } else {
            inline::get_remote()
        };
        frame.load_html(html.as_bytes(), Some(page));
    }
    #[cfg(not(feature = "inline"))]
    frame.load_file(&format!(
        "file://{}/src/ui/{}",
        std::env::current_dir()
            .map(|c| c.display().to_string())
            .unwrap_or("".to_owned()),
        page
    ));
    frame.run_app();
}

struct UI {}

impl UI {
    fn recent_sessions_updated(&self) -> bool {
        recent_sessions_updated()
    }

    fn get_id(&self) -> String {
        ipc::get_id()
    }

    fn temporary_password(&mut self) -> String {
        temporary_password()
    }

    fn update_temporary_password(&self) {
        update_temporary_password()
    }

    fn permanent_password(&self) -> String {
        permanent_password()
    }

    fn set_permanent_password(&self, password: String) {
        set_permanent_password(password);
    }

    fn get_remote_id(&mut self) -> String {
        LocalConfig::get_remote_id()
    }

    fn set_remote_id(&mut self, id: String) {
        LocalConfig::set_remote_id(&id);
    }

    fn goto_install(&mut self) {
        goto_install();
    }

    fn install_me(&mut self, _options: String, _path: String) {
        install_me(_options, _path, false, false);
    }

    fn update_me(&self, _path: String) {
        update_me(_path);
    }

    fn run_without_install(&self) {
        run_without_install();
    }

    fn show_run_without_install(&self) -> bool {
        show_run_without_install()
    }

    fn get_license(&self) -> String {
        get_license()
    }

    fn get_option(&self, key: String) -> String {
        get_option(key)
    }

    fn get_local_option(&self, key: String) -> String {
        get_local_option(key)
    }

    fn set_local_option(&self, key: String, value: String) {
        set_local_option(key, value);
    }

    fn peer_has_password(&self, id: String) -> bool {
        peer_has_password(id)
    }

    fn forget_password(&self, id: String) {
        forget_password(id)
    }

    fn get_peer_option(&self, id: String, name: String) -> String {
        get_peer_option(id, name)
    }

    fn set_peer_option(&self, id: String, name: String, value: String) {
        set_peer_option(id, name, value)
    }

    fn using_public_server(&self) -> bool {
        using_public_server()
    }

    fn get_options(&self) -> Value {
        let hashmap: HashMap<String, String> =
            serde_json::from_str(&get_options()).unwrap_or_default();
        let mut m = Value::map();
        for (k, v) in hashmap {
            m.set_item(k, v);
        }
        m
    }

    fn test_if_valid_server(&self, host: String) -> String {
        test_if_valid_server(host)
    }

    fn get_sound_inputs(&self) -> Value {
        Value::from_iter(get_sound_inputs())
    }

    fn set_options(&self, v: Value) {
        let mut m = HashMap::new();
        for (k, v) in v.items() {
            if let Some(k) = k.as_string() {
                if let Some(v) = v.as_string() {
                    if !v.is_empty() {
                        m.insert(k, v);
                    }
                }
            }
        }
        set_options(m);
    }

    fn set_option(&self, key: String, value: String) {
        set_option(key, value);
    }

    fn install_path(&mut self) -> String {
        install_path()
    }

    fn get_socks(&self) -> Value {
        Value::from_iter(get_socks())
    }

    fn set_socks(&self, proxy: String, username: String, password: String) {
        set_socks(proxy, username, password)
    }

    fn is_installed(&self) -> bool {
        is_installed()
    }

    fn is_root(&self) -> bool {
        is_root()
    }

    fn is_release(&self) -> bool {
        #[cfg(not(debug_assertions))]
        return true;
        #[cfg(debug_assertions)]
        return false;
    }

    fn is_rdp_service_open(&self) -> bool {
        is_rdp_service_open()
    }

    fn is_share_rdp(&self) -> bool {
        is_share_rdp()
    }

    fn set_share_rdp(&self, _enable: bool) {
        set_share_rdp(_enable);
    }

    fn is_installed_lower_version(&self) -> bool {
        is_installed_lower_version()
    }

    fn closing(&mut self, x: i32, y: i32, w: i32, h: i32) {
        crate::server::input_service::fix_key_down_timeout_at_exit();
        LocalConfig::set_size(x, y, w, h);
    }

    fn get_size(&mut self) -> Value {
        let s = LocalConfig::get_size();
        let mut v = Vec::new();
        v.push(s.0);
        v.push(s.1);
        v.push(s.2);
        v.push(s.3);
        Value::from_iter(v)
    }

    fn get_mouse_time(&self) -> f64 {
        get_mouse_time()
    }

    fn check_mouse_time(&self) {
        check_mouse_time()
    }

    fn get_connect_status(&mut self) -> Value {
        let mut v = Value::array(0);
        let x = get_connect_status();
        v.push(x.status_num);
        v.push(x.key_confirmed);
        v.push(x.id);
        v
    }

    #[inline]
    fn get_peer_value(id: String, p: PeerConfig) -> Value {
        let values = vec![
            id,
            p.info.username.clone(),
            p.info.hostname.clone(),
            p.info.platform.clone(),
            p.options.get("alias").unwrap_or(&"".to_owned()).to_owned(),
        ];
        Value::from_iter(values)
    }

    fn get_peer(&self, id: String) -> Value {
        let c = get_peer(id.clone());
        Self::get_peer_value(id, c)
    }

    fn get_fav(&self) -> Value {
        Value::from_iter(get_fav())
    }

    fn store_fav(&self, fav: Value) {
        let mut tmp = vec![];
        fav.values().for_each(|v| {
            if let Some(v) = v.as_string() {
                if !v.is_empty() {
                    tmp.push(v);
                }
            }
        });
        store_fav(tmp);
    }

    fn get_recent_sessions(&mut self) -> Value {
        // to-do: limit number of recent sessions, and remove old peer file
        let peers: Vec<Value> = PeerConfig::peers(None)
            .drain(..)
            .map(|p| Self::get_peer_value(p.0, p.2))
            .collect();
        Value::from_iter(peers)
    }

    fn get_icon(&mut self) -> String {
        get_icon()
    }

    fn remove_peer(&mut self, id: String) {
        PeerConfig::remove(&id);
    }

    fn remove_discovered(&mut self, id: String) {
        remove_discovered(id);
    }

    fn send_wol(&mut self, id: String) {
        crate::lan::send_wol(id)
    }

    fn new_remote(&mut self, id: String, remote_type: String, force_relay: bool) {
        new_remote(id, remote_type, force_relay)
    }

    fn is_process_trusted(&mut self, _prompt: bool) -> bool {
        is_process_trusted(_prompt)
    }

    fn is_can_screen_recording(&mut self, _prompt: bool) -> bool {
        is_can_screen_recording(_prompt)
    }

    fn is_installed_daemon(&mut self, _prompt: bool) -> bool {
        is_installed_daemon(_prompt)
    }

    fn get_error(&mut self) -> String {
        get_error()
    }

    fn is_login_wayland(&mut self) -> bool {
        is_login_wayland()
    }

    fn current_is_wayland(&mut self) -> bool {
        current_is_wayland()
    }

    fn get_software_update_url(&self) -> String {
        crate::SOFTWARE_UPDATE_URL.lock().unwrap().clone()
    }

    fn get_new_version(&self) -> String {
        get_new_version()
    }

    fn get_version(&self) -> String {
        get_version()
    }

    fn get_fingerprint(&self) -> String {
        get_fingerprint()
    }

    fn get_app_name(&self) -> String {
        get_app_name()
    }

    fn get_software_ext(&self) -> String {
        #[cfg(windows)]
        let p = "exe";
        #[cfg(target_os = "macos")]
        let p = "dmg";
        #[cfg(target_os = "linux")]
        let p = "deb";
        p.to_owned()
    }

    fn get_software_store_path(&self) -> String {
        let mut p = std::env::temp_dir();
        let name = crate::SOFTWARE_UPDATE_URL
            .lock()
            .unwrap()
            .split("/")
            .last()
            .map(|x| x.to_owned())
            .unwrap_or(crate::get_app_name());
        p.push(name);
        format!("{}.{}", p.to_string_lossy(), self.get_software_ext())
    }

    fn create_shortcut(&self, _id: String) {
        #[cfg(windows)]
        create_shortcut(_id)
    }

    fn discover(&self) {
        std::thread::spawn(move || {
            allow_err!(crate::lan::discover());
        });
    }

    fn get_lan_peers(&self) -> String {
        // let peers = get_lan_peers()
        //     .into_iter()
        //     .map(|mut peer| {
        //         (
        //             peer.remove("id").unwrap_or_default(),
        //             peer.remove("username").unwrap_or_default(),
        //             peer.remove("hostname").unwrap_or_default(),
        //             peer.remove("platform").unwrap_or_default(),
        //         )
        //     })
        //     .collect::<Vec<(String, String, String, String)>>();
        serde_json::to_string(&get_lan_peers()).unwrap_or_default()
    }

    fn get_uuid(&self) -> String {
        get_uuid()
    }

    fn open_url(&self, url: String) {
        #[cfg(windows)]
        let p = "explorer";
        #[cfg(target_os = "macos")]
        let p = "open";
        #[cfg(target_os = "linux")]
        let p = if std::path::Path::new("/usr/bin/firefox").exists() {
            "firefox"
        } else {
            "xdg-open"
        };
        allow_err!(std::process::Command::new(p).arg(url).spawn());
    }

    fn change_id(&self, id: String) {
        reset_async_job_status();
        let old_id = self.get_id();
        change_id_shared(id, old_id);
    }

    fn post_request(&self, url: String, body: String, header: String) {
        post_request(url, body, header)
    }

    fn is_ok_change_id(&self) -> bool {
        hbb_common::machine_uid::get().is_ok()
    }

    fn get_async_job_status(&self) -> String {
        get_async_job_status()
    }

    fn t(&self, name: String) -> String {
        crate::client::translate(name)
    }

    fn is_xfce(&self) -> bool {
        crate::platform::is_xfce()
    }

    fn get_api_server(&self) -> String {
        get_api_server()
    }

    fn has_hwcodec(&self) -> bool {
        has_hwcodec()
    }

    fn get_langs(&self) -> String {
        get_langs()
    }

    fn default_video_save_directory(&self) -> String {
        default_video_save_directory()
    }

    fn handle_relay_id(&self, id: String) -> String {
        handle_relay_id(&id).to_owned()
    }

    fn get_login_device_info(&self) -> String {
        get_login_device_info_json()
    }

    fn support_remove_wallpaper(&self) -> bool {
        support_remove_wallpaper()
    }
}

impl sciter::EventHandler for UI {
    sciter::dispatch_script_call! {
        fn t(String);
        fn get_api_server();
        fn is_xfce();
        fn using_public_server();
        fn get_id();
        fn temporary_password();
        fn update_temporary_password();
        fn permanent_password();
        fn set_permanent_password(String);
        fn get_remote_id();
        fn set_remote_id(String);
        fn closing(i32, i32, i32, i32);
        fn get_size();
        fn new_remote(String, String, bool);
        fn send_wol(String);
        fn remove_peer(String);
        fn remove_discovered(String);
        fn get_connect_status();
        fn get_mouse_time();
        fn check_mouse_time();
        fn get_recent_sessions();
        fn get_peer(String);
        fn get_fav();
        fn store_fav(Value);
        fn recent_sessions_updated();
        fn get_icon();
        fn install_me(String, String);
        fn is_installed();
        fn is_root();
        fn is_release();
        fn set_socks(String, String, String);
        fn get_socks();
        fn is_rdp_service_open();
        fn is_share_rdp();
        fn set_share_rdp(bool);
        fn is_installed_lower_version();
        fn install_path();
        fn goto_install();
        fn is_process_trusted(bool);
        fn is_can_screen_recording(bool);
        fn is_installed_daemon(bool);
        fn get_error();
        fn is_login_wayland();
        fn current_is_wayland();
        fn get_options();
        fn get_option(String);
        fn get_local_option(String);
        fn set_local_option(String, String);
        fn get_peer_option(String, String);
        fn peer_has_password(String);
        fn forget_password(String);
        fn set_peer_option(String, String, String);
        fn get_license();
        fn test_if_valid_server(String);
        fn get_sound_inputs();
        fn set_options(Value);
        fn set_option(String, String);
        fn get_software_update_url();
        fn get_new_version();
        fn get_version();
        fn get_fingerprint();
        fn update_me(String);
        fn show_run_without_install();
        fn run_without_install();
        fn get_app_name();
        fn get_software_store_path();
        fn get_software_ext();
        fn open_url(String);
        fn change_id(String);
        fn get_async_job_status();
        fn post_request(String, String, String);
        fn is_ok_change_id();
        fn create_shortcut(String);
        fn discover();
        fn get_lan_peers();
        fn get_uuid();
        fn has_hwcodec();
        fn get_langs();
        fn default_video_save_directory();
        fn handle_relay_id(String);
        fn get_login_device_info();
        fn support_remove_wallpaper();
    }
}

impl sciter::host::HostHandler for UIHostHandler {
    fn on_graphics_critical_failure(&mut self) {
        log::error!("Critical rendering error: e.g. DirectX gfx driver error. Most probably bad gfx drivers.");
    }
}

#[cfg(not(target_os = "linux"))]
fn get_sound_inputs() -> Vec<String> {
    let mut out = Vec::new();
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    if let Ok(devices) = host.devices() {
        for device in devices {
            if device.default_input_config().is_err() {
                continue;
            }
            if let Ok(name) = device.name() {
                out.push(name);
            }
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn get_sound_inputs() -> Vec<String> {
    crate::platform::linux::get_pa_sources()
        .drain(..)
        .map(|x| x.1)
        .collect()
}

// sacrifice some memory
pub fn value_crash_workaround(values: &[Value]) -> Arc<Vec<Value>> {
    let persist = Arc::new(values.to_vec());
    STUPID_VALUES.lock().unwrap().push(persist.clone());
    persist
}

pub fn get_icon() -> String {
    // 128x128
    #[cfg(target_os = "macos")]
    // 128x128 on 160x160 canvas, then shrink to 128, mac looks better with padding
    {
        "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAIAAAACACAYAAADDPmHLAAAACXBIWXMAAAsTAAALEwEAmpwYAAAAIGNIUk0AAHolAACAgwAA+f8AAIDpAAB1MAAA6mAAADqYAAAXb5JfxUYAAEezSURBVHja7L13nF7Vfef/Pufc8rTpXdLMSKMuUEMUgYUARRSbYmwnjhOX2OuS5rgk2WyySbwbx/klu5vmeG0ncby244q7HWMwYJqpEkLISEKj3kaa0fT2lHvvOef3x73zTJEACQYQxJfXoJFmnue595zP+dbP9/sV1lr+M1zjO56ussNDB53RYZx8HlEMEFGEsBYrJcZ1sKkUUTZLVFmJrK5ekF2xYuS1vi7Oa2qT9+713H37l9j9BxYVjuzv8I52z/V7e+v1+EjOKRRSNgxrVWSwWITRCGPBghAgpAIhUEogpEKkvGGdyiAqanrHmhv22/a5uzMLF23RHQu3pa65ZttrZc3Eq1kCFJ7eWels376msO2Ji1Ode5eaE6fmqLGhm5QJkLigFMaRCCERUiSvsiAAJMk3yZ8abPIzK8Ca+O/GQqRBGzSayMtgq2sQ8+bsDlYuf8Bfd8kP9ZpVd6fbF+hfAOBluEoPPTwvuv++q+WWLZeJgwc6vNGxNyAVwlUIpbBKIqyINxGLFZPbPLnZCRDO9O/Cxhs/41UTrxCYBBAaQoOWlrC2Hrts2aP2yitukxuu+nLqwgsGfgGA2dz0p3dUhz++6wbv3p9ew6H9i7xSuAnXA9cFKWZs7MRlkn8TM7bwrJZlxmvirbdCIGaulwWMhjDARIawModdsWqbueHaz2be857P/wIAL+Iqfv97F9nv/vBN4sktF3mjo2+Qng+ek4jv8++yGIS2mCjERhFRQxPmyiu/Jn7lzX+Vuvx1u38BgLM15j73+evEd779Fn/PnmVSyo3C87GOBGsQJjmJ4vy7b4NCkhiZFqzW2FKRyEsRXnbJQ+LXfu3PMm94wwO/AMCzXPl//uwb5Ne+8WvukSNtynU3Gt8HYRHWIqwEYbFCJHbceQgAARILVmDEhOaQSB3GQBAO0ZrVT/Oe93wkfdNN9/4CABPW/Fe/uoF/+/J73YN7OhzP22h9D2EN8XIKBBMGtjxHXf7yK4FJ78Kcbk9YA8UCJaHQl12+Rf7Oe38zteHqp/7TAqD40INt5p8+/Xvulq2XOo7cKHwHm4QmBK+9AJVNnAwjwBRCjHLQt1x/m/zQh97uv0Ju5CsGgPE//7P3ed/87q+4Yf46m06BcBAGjDJIwxndsNcCAIwQKCPQUiNNhBkPiZqasb/9gfelnsVrOHCg1zt2ZGTl1ZsWbnvVA6B4111L7P/6u//m792zhIqKDVIILAaERBiLFQbxGtx8ynItNhARYK0AoSAsYkoB0VUbHvL/+A9/hRWruwEeuO/Auj2d3ZefPDm+NJerHHjzL6/5eMeClH7VAiD/8U+8y/nyl97pWb0ZPxNvtpHxYggTL8x5r+fP5ZKASeJKiXEoQVmDEQppFIgShAHgMqBSPHPpDQ88s+nNX+s7Od42VtS1npK/DYramizXbFpTd8EKd+BVB4Dins6U/bOP/ZW/5fFLZTazYTJ485/jMkLE7qvUSCNBaNAhaEs+Xc3B5qXsbV/LkTnLGfeqiUyIL2PpYIVGa0tNdZbN115aYXWolq9wh2fr3l7yZFDxJ3cuEx/7+F/4PV1vtRUVsSX8n+oSSHTszmpABwRemq6GxexrW83etpWMVM6j5Eq8KAICPAlYhRHRTHU4KqRbsbezlFuy1B877wFQ+NznrlN/97d/4EbRdeQqwepY9JXDq6/tywqD0AJMgBEufZXNHJi3kj3tF9Pd0E7R83G0xhLihhZhBRIVxxCERlmJmRLwEoAQdjTSbvV5LwHyH//Eu9wv/tt7XcffaP0UWJBWYqVOEjWv3hM9bYMBa2TstorEdrFx9lBYGM5Vc7D5cjrnr+V402LGspUoY5EmxAtLCASCONilhUBYkNZihEBLEye2Zny+kkLv3hXVrrjAGTgvAVD4/T/6UOrbt/2KzGU2TJ54A4IzPNCr0ZlLgKBV2YC1WKTWYAxFP8vRlkV0tl3EwbkrGa6sw0pQkcYLS6e9n4U4vJ1kL8swe/a1GlXKqXjmmaBq+XJv+LwCQOF3fveP/Nt/eLPIZTcgHIQwrxFpP7EzOkkQSpAWTISMNNpxOVbbzt72i9jXtpqBmhZKro8XhkgdIvXEVs/aNSqsW71/XzG9aHGqcF4AoPDbv/XH/u133ihzuQ1WOIlrZ19bQZ0J80WHCCS9lY0cmruMPW2Xc7KxjWIqi9QWYQ1eoBEkhpyVSGZX+wkphoLArQNeeQAUP/Sh30/96Cc32srsBmsVJGIRq2Ip8KrY2zOHoOJ4RbzpGMNYpp5jrQt5pn0tB+dcyFi2GoHBiTQqDFDWxEkrLFgXhAGhMYnGn0UzBKVUsHtnVLviwhdmD8wKAIp/+rEPeD/43puoqNwgRJK8scQP+6rZfAPWwQiLSLYKYSCKEMZSdNOcaFnMrvZLODp3Jf3VTVipUDrCi4rT9ibe/ORvYjKRJWZTCk1RBdJxKvbsKeWWLTt31/BFA6D4j5+6VX35y+8UuewGIxXKRDHL8lWn4iVWgDQ2jlVEGus69FS3sad1DfvbV3Oqto3IzSBNgKMD0PJ8kVujxnjVhw4W1YKOcwsVvygAlL77vYvU//3k7zlZbwO4KGOx0r4qLX2hQeg8CMNgRQsHmlewv30th1qWEHkVGBHhRhqp8whrsKjzIGM5SVeTSgwVCl4dMPCyACB8cnut+ItP/A9Xik1WulihsQikVedJkGciJy+nLJQsL1i8fgm50xrGUzkOzbuA/a3rODJ3OUOVDYDAiSKULqISt8yxSWjXMqnnX3HPJALjIKQ450TRCwaA/ZM/+WtvZPQWm3UTxs6zsW1fQW89jtLEKim5P2EBE4AxaNfnWGMH+1tXcqDtIk7VNGNkCmVKuFFpistvk+UWWGTi2byc3o1NDOkzfaYA60znoLzUACh89MO/n3pm1woqqpNFOt8MPZswL9wkCxfFDKMoQktJf+Uc9s27gINtaznauBjjOhij8YzGmjzCCqxIgIOYlBzCYm3y3lMDQi8xECwWx3GRQmBm+XydMwCCr3/tCu+7P34jFZUbrCwhtH/eufkWCUYiRBGiGJxj6RoOti3jmfnrON6yjLFMFcIapImQYYA4vg/b2IqsqMJBAB5CmCn27ITGTyjixmJ0iDVg0FgbvXRAsCDOwrAWsf/90gEg2L/Xs3/3j38gUmIjSNCx7n/5CRwzReFUvW4RNgCtKfmVHJ0zn31tF3Fg3oUMVzYTKoEbhbhRESl8HM+DsSFsvp+wWzNQqmOo9wT54iBhKSAISoRhkUgblOuglML3M+RyVVRVN5HL1ZJOVZLy0wgp0TpCa40x5gzq8KVVG1KJ4MSXvvPuOb/xli++JADQ//v//FGq99SbqcjEIlXETshLvt3lHMJEMFUmaxnHHAwSaTSYECt8umoXcmDehXTOX01f9XxC10PaEKEj0lbhuCmksAyO9NJ35BgDzzzC+PFd9PefoLM4xmBx/OwWT6XxUz6VlQ3MaV5MY3MbCxeuoamxg0y2Gm00URRh7UTZmZwwJ7AvAa3dpsSod//PHigNH2v2P/SRv5lVAJS+/e2LnbvvvlZkstOA/XKcfWHiCk4rBCap+JHCIiINNkSiGKyoY//clXS2r+VY0yJKXhZPGwzxaXeUi+unGR8foHPPE+za+Qh7929ldLiHFsADFJADhihXlj3nFekC0XiB8fEhTp7cl6yHQ0NDKx0LVrJq5dW0zb+ATKaaQIdEkU4yiKYcHJptp8DxvavsP//rVeGNN/29u3BRMGsAkJ/6v7/nOM7GmM3z8lr6VigQEcJKlBFgC2Al+VSWw02r2TX/Eo61LGU0VxsvQpJ1M0LhOD6ukvT2HOapHXfx5JN30dt7vPze1UCFkGgbp3YrEFQJGDobppQQU5AyYSFEnOo9xKneQzy25Yc0NbWzcuU1XHTRDTQ1LUBrTRiFL43hbEH7HqlTpwj//lPf4tOffOOsAKD4N//rbd7hAx1ka7BEL7vOF4SggahA6Kc4UXsBnW2r2Ne6lsGqJiKlcLTGDQ2CAItEKpeU79B/8hgP/uxrPLH9borFsRkhW6hCxkTNxMWLMNQB+eSdnlc3TUbjJtVTkvrGSnp6jtDT80Uefug7rF79S6y//BbmtV6A1pooKs66DJUWyGURd91+S3Dvreu8Tc9dyv68AIj27vPUN277NZGu2GCJM2AvzSm3CKOSdKvASoGIIrAh4HGyZk4i4tfQW7+AkuchjcXVETIK4vVGgXBI+WlGhk5y913f5JFH/4NiYeQ0SFkgA6QthMImln+cwHJsLBlOPe9dmynvNyEJkqJUa4DJuEyhOMpjj3+fJ7bdySUXX8c1m95JXd18SkEBa0xc6mZnI1sYN7xQWqM/89kvsOmaVS8OAJ/71w/4Q723iGxlbLhYNe3BZmf3VSxVZCmpmggREQxW1nCoaSWd89dyrHkR+XQ1ymiktmVixcSpM1iUA56TYceOu7j99n+hv//oc/hVUG2T7y3oxMBUKBzXp9lxcFyfkptGKokrFVYIrLVEUUgQFikURgmD4jQVMOUsnibmBYIoKvLoYz9k565HuPbad3L5+jdhlU8QFc7K1XvepUz2R6RzqG1bVwbf+d4t3lve9MMXBIBg1+6c8+PbbxSpXFz3Zl4qyy8ss2SLfhVHWxbS2baWg/NWMVxRA0ikDfCDAKzFqCgB4qQo9r004/lhvn/HJ3n8se8z6Z+IMxIxXBtLAC0g42ZJZ6vIZStIpyvwU1lc6UCmgvG6OViVwpMgpYuUEiEsxhqCUkCxOEohP0xf30m6ew4wOHQqOf32jGmbWFBYRkf7+O53/4E9z2zjlpt/l4bm+RSLxRdtG8T0MoMRDo5UBF/4/D/yQgHAF774bme0cIPN5RAYrEzQNSs2oI3r6nWEdnx6ahezu201B9pW0VvbSug6OJHB0RFaWFwtCKXFwSKmbj7gpzP0dB/ktm98nKNHd5ejc5Ni+fSrzktTn6ulorKadKoCx/NRQiFEXNtnpUAGeZzxIYKKeiIDylqsdZFSoZRLLpemuqoepTyWLXMIwxIjw70cPbabI0d20tt3/HSVYRWU082C3c88yNFju7j55t/jkkuup1gKsC+COR3vk4tFg5fG2blrQfid793iPgsInOcK+vDTO68nkwKpzynDZ4mTJUZapIlz4rGWjTlzaA1C0l/VQOfcNRxoW8WxpkUEXhZhIlwd4QUREo1AY61MOn0kxMnyxkrSKZ9ndv+Mr9/214yN9iU/mQiM2xmWOmSztTQ1zGGxmyalHIRQSY8Hg5AgpEUIgbQSgSI9NopJZ7FODiEsFosVNu4eYywRIcZatNE4yqWxcT5z5y7mkotfz9Guvezc+TBHjzwdxwIg4QeIOCxgYyCMjfXz9a//BUODXVy18R1oIeKAVmKenisEhE3AqgxCKczXvvIJzhUA9lvf/WV3YPQmculzSvTYsnGU8F2lRmiFMKVY9KVrONyyiD3zL+Vo82LGs3UAKB3ghYUpj6ETzari7RRM8T5iskY6lWLHk/fyta9/nDAqzPj8RAcn7lwmU0VL83xy1Y3kLLijo5ObLuSUZ4zpW1bEEkSYCH9snGJNFoPCsRZp41K2GFhTFI01hFGANhHK8ejoWMPChes41X2ALVt/xMGDO6Ys4YRiEjFGreWOOz9HNlfP+vW3xsahlS+QUBM/v7AWPA+1Y9fK0gMPrPGvuuqpswaA+PGPbhS+PyWFeg5aSADCIAJAhBQ9h+6a5ezsuIhDc1cyXNlIqFz8METqAvKcTV9Lyq9k2xO3883b/ppwCiNnJhSldGiob6WhcS6ekyUEvOIwgogJ2/+MSVYhEFIghSBTHEfrIiZdhbBx0yk7Ud1kxfTAmBCxMWcNYRggpUPL3KXcOnc5B/dv48GHbmNg4MSMbF78Bh0dF3HBiisIo4Q5LMPpts4L0gkSpQvwjds+wVVX3XRWAAhu+8Z6daxrHrkU1opzID5YhAkRkSFyfU7VN7F/7kXsbV/NqbpWQieNMBHKRPg6KtvK5xohT6XSPPXUXXzjG3+F1sGEV3/avaTTlbQ0d1CRrcYICE2IayAdRgn1a+rGSaRUCCmxGKyO0KGMi1l0SBjkKVQ3IvwMGSeFm07hp7Io5U2Li1ibZAynWMsxECRLV1xGW/tyHnjwG+zYcW/5tFpgzpylvOOdHyObaSDQYwjrn7XUfc4dkUDKg0ceuTE8sDftLlxSeF4AiB/efrOQaiNn2HybRFBE2ahJWqxFAUiHwVwdB1ouZF/rOo63LCKfziGNiTdeF1BJ6beViUFkz6HdixX4fpojR37Ot775N1M2//SrqrKJpuY2XMcnMhFSOhgFmVIRZSxaKaQQSBkbfaXSOOODIxSKY5SCEmEpjzYR1mg0BmWhR0hGpEJJl3QqTU3tXGprm2huXEBd3RyyuRqEEGhjMNi4JAxiqWElpVKAn67hlpt+j/b2Fdz5k38nKI1QV9fKu971cSoqGglKRYRQIMJZcbmEBSsdxOAw5o47PswHl/zNcwIg3LMnxY4da0RKJUGf07tiWSuxMkBGsY4t+FUcmLuIg23rODB3BaO5akBiRR43CKbpMVuunpFM9uw7O72mHJ+R4R6+/vX/j2Jx5FlOiKCutpn6+nkIoTDGIKVM3FhLKgwxEhyh0CZkZGSQoZF+ioVxtA3L0kjM8OjjsLFlTIeEOiQM84yM9nPkSPyZFRW1tLR0sGTJOubMWYbnptHGMNFdbMJ7MjqkhGXtRddSU93Mgz/7Ljdc/27q6+ZTCsan8Clnx9eeCFArTxHdfe+7+eCHnxsA5qf3bPZGRt9AZTrxZ9U031RgEFGJyPU4XreIve1rONB6AT11bRjpoHSAGwVJRWwaIcIXnTkQFpAOUkm++/2/p/fUoWSdBDOrm+tqm6mtnYsxFilteT2tgEwUkjKWSGiGB7oYHOqllBiP8nlSMxZIWagVklOYGMDlnoKW0dF+Rkf72bt3K83NC1m18mo6Fq3B97JEOrbmbWK8WkKKBUF7+xre3r6KdCpNKQgpNw6Y5QQRVoCncPbsXRps29bmrVt39FkBIB948CrrS6SR5ZRl+ZasoaDSPHXBJjrb19NTP4/RdIpUBI4OEVGcgIkp0BYpArQwLzp8bIUg7ad57LEfsvPp+2Pvwp4efaupbqKquhFjNVLIaauggJwuMZYfoHewm2IpP2XTxYx8vX0WGWSptDAuYLy8+VO9k9iy7+4+QHf3AeqfamX9pbewZMl6jDCJf6/KnxFGJZTjxNsgSrOfHUyeRyAwwkUWRzD33vte1q37H5wpH1nqfCYjdu9ZgevFnhYzbABjGKio5u7LfpXDrYsIXUWmFCBNKTacptW3xb64tM6LfgjH8Th16gh33PGZaQ7U1CuXraGqqiEJwtnpUV+pcKOIkVPH6Oo+TFDKI0/bttOF/0Q0P5ryBZYaK+J6RwGgsEkzKzttOSV9vcf40e2f4cc/+TT58SF8N5fUH0xwGzRKpROvQb4kSTZhbDnZJRwPHnn0l59dBWzddpEaHXqDyFU86ykQVuBFARpRJoUwxRueFg0Q8kWENst91nCk5Cc/+TyjowMz9GP8OX4qS3VNE8bapBBVlSOBSkqiqMRY13680V4kAo1ETknkiOS9DJYoCb04wsHzPHw3je+nkUJSDAoUC6MQBnjWEjufOlEzsqyOxJTTZ7Hs2f0YPSeOcsPr30dHx2rCIK4GVji4jioTWGO+4Wym20VCmkmSVJ6L3L9vRdC5N+ctXTJ2OgAeeewKIV75YoeYch3ftOdWcODg4+z4+U+nCK3JHLxSDtVVjUkixU7LyEnpUAgL9BzfS01xFAlnTGbHGw85P0tLSwfNTYtobGzFT2VwnRSum0IgCKMipdIoYWGMrqE+9o0PcOj4PvoHuqY0vpCJrp+4l7hMbnDoBN/6zv/h2s3v4uKLXk8QRUjXQ0qVSCCDTcLQAjvtCMxerlgiRsaQW7a8iaVLvnwaAJxndi3Hc3nlEWDAunGwzQb89N6vYHSYhHUnUsbxgldU1OI4Xsy2sQYlnVjnK4cwHOfo8X2kSmOkRdz4W07t/YslwFKVrWH5kktZsmgtFRVNIAzaRlgjsVYnsgEcN43nZXAqmmlqXsratEchCDh+4iD7D25n/+GnGSoMl6VhOKFeko7VYVDgjh//KybUrF9/C46fwso4YjcRlBKJA1mOD81yRxUpQW/begvvfMd0AIRPbq+13d3NsVHyyl8Wgeem2bvnMTo7tyb18hLQZVHr+zkclWJsbIz6+nqamhqxWIaGRxgZGKbr5AEUEdUJF89OWeAQg0KxavklXLT6WnK5OsIoIIhKU9TWzPSnxlooAZgQRoq4AhY3tbOkZQFjq67imc4t7Nr9CPlgHBcZ9w6w8ck2QhBaw467P09tMMZFa19PEIVYJbFuCqEcrHRAqtjJEAr89KxaBtZxsLv3bjjdBtiz80I3n7+BXPY8UAECKSIkLo8//oPJUyB04mGIuAWrdKhvrOd9738Pm67eSF1tDcYYhkZG2f7UDr7/hX9j2wP3x9W6ydkXSAIMjbVzufzSG5g39wJCrSkFeabPD3guu3ry1wwQRPFZ99MVXLLuRhYtWMWjW3/MoePP4NrYTLQIlLW4CQS3Pfgt6pRHa/tKgmlNI1RSfxASZWrxWpclNg3lcocXpRYcF+fk8eawszPnLl06Vlb40a69y8R50oXbCFCOx4nuvezpfDQR1zL5it0vayzr1q3ha1/5N37nt95DR0c72coMmVya1jnN/PKbb+bfv/ddPvn5z9PY1EQeg0SQx9DWspibbvhN5s69kFIYYMxscPoFxhhKYZHK6jlct/ndXLpmE2ZKWssm2l4g0VZz3yPfY3ikB9fNxgMtpBNzukSAVBX4zQvA8SbT2rNRWS7Ajo2j9+67fJob6Ozft9A4akpp8+x6o+eoqXCUz/an7iUIimXOj7IGx8acgEvnL+Az//SPLF6+jGKxRPexE3QfOY5EIn2PsfE8+VLE63/11/mX//ghS5etYATNotZlbP6l9+D6WUphgUl2k5h+w8/29Tz3DYZIlzDWcum6W7h6wy+DVAkIRNnlVMB4cZRHHv0R1hQQwkHaGNxGpKC1A5OpnBLosrOQGUiSXDbC6+y8fLoKONk1RyiHF168EKPUGIvWplzNImSc8zYWpHKeNe4ft4BPwjLCEBYGObTrISoQpGLvmQUNTSxdsRwvl+ZX3/VfaF+2hJOHj/Clv/tHHv7pAwhjaFnQylvf9x6uueUmxosFBocHmL/sAv72S/+PT3zoj7mwbSOO52DCElLI5J3NFBJJnNuP8/GTDl3Z3RUCJdW0uEHcQEJgdIjWcTJIYAllyOKFl2Gt4b6Hv1t2MU3Z7oenT+yhZutdXHLJTWhCQOA2dWBydaCjcgOpWTQDEVIQHTy4Tk4AoNS5J8XwULUQkhf6WdpEGA1VlR7NzVlqa30yORfXcfA9QV9/iS2PnWCiUcJkBjimMNnIoPJ5TDCCjQJOHd+D23OYOiwVFRX89n/9I677lbdS09yIqyS6FBFFIU898ijf/vyXqKqqxlrNzx7Yy0MP/JTf/OhH+O0//xglG5EfHaZ9ySI+/qlP84N/vh0dBFPcL1EmUWhjyFSm2PCmq3AzMXfFWpVEFCyu49J1oIutdz6JdAQSgbGGsBQiXKhprKF2Th21TTVkczlSWQ8ch+vS1/K2U79BqTSGEJJSMSAKAgYHeunuPknX/i6kX6I0Bqq+DVvTAiYqB9SsMJPC+kX7hgahXEzX8aVlCSBP9MwRY4Wb8PwXcPY1YeBQU+2yek0T8xdUUpH1UK6Js2KhBBExtzVDd/cwB/bncb0p6VNjMV37keODlIiQRuK7KboO76BkImpqavnbL3+R122+lshoolKAtZZQCBwhOLz3AL7r4bguY+N5Usn6fOYf/hGhHH77f/45wUhEsVhizqK5XHTtGh793mO4KXVa8MEmVtb8FW1kan2iwKKjIMmoWdIVOYIwREQaqTwirZHCsuSiRSy7YgXN8xupqKwAYQgmXmfBSsGiNR0I6eA4CtdRaCOIoggdlgiDgPGBcU4c7WfnoTGOnCjhCQcpX5r6Cyslbt9gWxkAzqnuRkKNTXPOfL8wgLa2FFdfPY+qKg8IGRst0NcXkE671NZKjFE4juDiS5o50XWIILQIGQtYKSWmthFTKOBEo7FLJCyneg5TAv7wLz/BZZuvpzAyzLc+/3nuueMnXHTppbzx7b/OvPltbHvsMaTroK2mVJpM7KSAf/nbv6V9UQevf/vbiUbHKYQFVl15IYd3HOLkkR5c12UqP0cqQTSuueurdyOVobaljnWb1hJJgw0Vj/3H/RzfewzrK3RUIlOd4aq3XMOCtfNxfcnIwCidOzrxHJeWBc2EGBwpKPQVuPee+yiNjrH34HZSlQ5LV1zAqkvW0zK/jcga3ArF4tXzaL9As/PpAbY+3o3WAqlme/tFEhAaSet9+30HIOruaXamGUNnR/3SoaGlJcu117aR8hRhaNizZ5zt27sZ6C+yfHkNv3RdO4QRWgc01GVYs6aBRx89ies5ZQKFqKhHzvcxXYeQxTHGg2G6e46y/tJLuOFtv4wJQ+78wQ/48z/5EyqApx98hDu+9k2uuH4z3QcO4fs+QVAkMmHiMIkkXmj5+//+56y55Aoa29sIwgJ+2uXia9dy++d/Ms3aEVZiJWij2b/9IDofsuBizbprXZTS9B7sjUU/IJXEq0pz4/tuonFRHbZkeOqu3Wy/bzunuvpYecWFtC5pRQdFrBSEkaZzSyfjg6M8s/dJHut8GB9orK/n+je9iXd/+MM0trYyPj6Kg2Td2jrSacUD9x1PYgizCwAhBDooEg0ObpEAdmCwFinOImw/UQETkyI9R7B+fROpVNwzb9eefu67/xijoxHSUaDkFMacIgo1K1fWMWduNo6HJ7FqtMb6lTB/GaKmgbGRQUajEm9829vw0ll0UOBHX/kqDlCZriCXq2BscJj/+PI3GB8bR0pJFATIGSapAqI8bPvpThAGaSVhoGm9YD6tS+YShuGkdT2l64fnOTi+oqmtHqViVdP5VCfCWBzPAW25/Kb1NC9sxkSGpx/YxT3fuI+R/hE85SDdKTWMJja6lLRoEbD8gsvpqJuDD4z29fHpz32OG2++mS2PP05ltgptLaUgYNnSetauqScKI5hCcn3RjOwk32DDiGigP16z0shIpRBnY/tPSocoMsydl6W5uQKtQ0pFy56d/UgpUE5i6E00XJpIDlmL4wiuuHweKd8SGZuAKp7kaZWH07aCfiPIuS5rrriCKNT09/dzeN8+0gnHzWJRjkMmk0FKgbGaKApOA2sILFm6mq69PRzvPI7juhhrUK5kxesujAtMjSrnHaaKN+VKmjvmIaVmZGCcgzsPo1yHKAppaK9n6ZrFBGGJsf4iW+97AtdRcWpXUM6+TVUvxVIRay2O8lm28FIEECEJhGD/oUO8+13vYs++PaRSKYyVRGHIhaubqKtLEbPnZokrkBiWwhjU8EgMADk2mgNxlu8/Me0gBoBUGiEFo2MlxkYEUj23CokiS0uTz0UXN2DCOE5uk6yhNCFCOhwaH8Cd10Jt6zwwhpHBYUYHB3ERCClPWwijNUZH05Z8wsBpaGxD5zU7H94V180JCMOQecvnUj+3jijSp8X+tDZU1lfQ0NqAEHB45xHG+8aQKqaPt61ow8k6CAlDPUMUh0s4aoLhJDDaTPPfhaBMHNG6SFPzfLxsDScwFJMTebK7mz//0z8FLFI4GKvJZAzLltVizUS52SxyBKzBGRuLAeAUi345i/Wcn5Jktyw4jqSqKp0ccJdIa7QNpve4nRG2lEllcRBFrFzZxMKOLEEYS4i48VI8mHFwuJ+K5mZy6QwWQxiUiHQUn347SVObYO9qYxJ/fvIuIyDjZaipnot0DMd3H+NUdy+O44KGdMZn4ZolRDZAGTktl68jTeuyNnJVKUpFzf5t+2KKizA4wqF+bn3MOLISY6L4bs6oq2MHUkqBTG7bGouXq6Ci/YI4pyCIeywBd/3kLh5++BHSaQ8Q6Aja2nKkMhJtEgaSmA0IxCpFFIuJBIgih6Tk6WzjelIqPF+ClSgVkU57uK4oU6WEEBhtyw0jdWQYHg1QKi6KUAIuf90cqisVJkpsCwHWavKjw9TV1eEopzxiRZTX2J4WXzRWTwPaxBpVVDSQTmewwlIYL3Jo+xGEEydbdRTRvmo+2WwKY6Mpwlrjug4dqxagHMmJw32cOtSNch0wAukpMpVpjDVYaaluqMZNOwn8YgDLJPtnBUh0suAiqT0wiNo2Wi/YcFqY1AI/+P4PUEolkk1QWeVQV+NjJuYo2dnQAnFXFxGUEthbI88uzjg1566RMsL1BYNDlie39RGGYko3q2QmDoA0WARPbulnbMygFGgdUl2V4YrXtcQVN1aD9TDGEITjeL5f5gZoKRgTEscqHGvQcrq1qnV0Rphmc5U4jou1IJXi4K5DBOMlUJZQa2qbqmlubyGMorLvoENLfWsdzQua0JHlwBOdBIGOM3OJj+m4DikvTX4gZNcjuxGBRZUrkWIJYpMCEpvkNoyMYtDUziWoqqWlaT4pLzst8gzw6GOPMjw8gpSxreMoRV1dJlEDs0USnKBNmBnZH/v8p98mGStjYWRE8uQT3Xz/e/vZvas/CZnK0yJPGInrKPr7x3nk4ZNYEYeEw7BIR0ctq9bUEQY2SZ1CpEvliKS1FuX69CvBgBBYoXDNjMycPbOcchy3bIw5jkP/yQEGjg3gODJRY4q2C1snS8esxWrLoos78Cp8RvtGObbzIGoqR8JaguGA7fdt51uf/AZP3rUda0GrSSNtQv/HNSMST0gcJLaqDls7DxtZKnK1VNfPO+2+jx8/Tk9PD47jlB+spsab9UmpU/ufgVLR82RBp1CMNELENKif/ewYDz/cS7EocD03eb2ZQiGM+fETNCfHl3TuG+DnT/WhnBQWQWRKXLxuDm1tGaIwnp6hIzONHZ1UHtAvBN2OLJeLPW/mU7mTqVRh0aWIY53HEDKu+9NRRMviOfhZH7RF24hcTY6OlR2A5MjPDzA0mI9PozAgLcpK7rztJ9x72/0U+gN8P4VWEdKoaWniSWM5lpamoh5TPw9D3MTKdbPU1jScds+jo6P09vaW1YC1gkxWISf6Is1WNFCAdZ0YAJHnRdgJStLzdcVwypy7IIxdKqmiuFslUzjYE0tgk8lZMbcbx5E8sbWb48dHcT2J1QJXGTZc2UIu42K0QEqXsbGx2GUlHis3kQcflZIe5RBIhRQThEdZ5r9ONH8QMEW3k7CEBMc7TxIVIySWyGiqGyqpmVOD1hodGFpXzqWmrobSeJE9T+1DEZ8+kRSjGgul0ZCUSiMdiREmLvyYEOUitnfi4s/4fBmlMLVNGOkirGHCYsmmq87gpltGRkYSilu8jr7nIJ3ZajWfPIcA4/sJANKZcc5ax8Q+syCum5tswToVPOKMfwpi4zDU8NCDJxkbjVBSEWpNXW2WK65owpEOmWwFQVAqxy3imnyZ6FUYVZJeJSkJB8cKhJoev5xYuqHhAfSU+IBUkoHuAYb6hpGOwBrwfJeWBXOIdITreyxftwzhCnoOddNzpBfHjZtPYFXSQNJOSoQJb2TGbB/sdGKyEAohnWQcroijjsJSUVF7BtM1lpzlugdMXLI2W17gxPEQApPJxgBQVVUjcR5kFgsTRPwgZf6+naRNOw70DwQ8+kg3VsaSJwwCliyuZOXqBqRwmNr7VCkVL3rCDLIC8krRq1xGHAdPyokJfeWCagmMjw0QRuOTaWapKOQLnDrSi1RxVxJjLHM7WrDW0LCgnub5zWgdsu+J/diiiUvYznVNZvy6kPKM3T88zzvjy9PpdHIeDQJLEGiMnk3tHzOkosqKeEfc6uqhSTbtLOoZY5J2frHrpyNTnp7h+Ia9+4bYuWMQ34259ZEJufTSFpYvbadYDE5jYRhrk+FT8VdRKfqUw7ibwaLKJRcTxemF4igjw/2T+jR5n+6D3WUGgNYRtS11pHMZFq5diJN1GTk1xuFdR1FeTD9TRmKkPsvlFUlvwEmfPQ5Unf76IDy9i1sqlaK+vj5hKcXSozAeYvQszhtIUsKitjqRAPX1fdaeXTMCcZod+XzsuYkKWU0YxMajSPwp5Ths3dpNV9cYjutgNAgR8TsffCcNDU2USsF0DyRh6Eo70dMrbts+6KUYdBR6SjWjQBAYw+HDneUqIYFFSUVv1ynCQjGOVRhI53w6LlxA2/I2hBEcevowY4Nj4MgXcPotxfw41hqsMEgkYWQIQzOVXgJAPj9ymgvW1NREc3MLUTTJAejtz08LdL1oDWBAes6dXnXNhji00tzUbZQ6SyUztVGUfg6jcVL/Cwn5QkixGJX7KsSpYEsQwc9+1kU+H+BIl2KxyLp16/jIRz5MKcn9u66H7/txxLA8jmXS5BPKZdRPMRA3RmGi7sYBDh99ukzEkBikchjpyzPaN45Ukzez9po15KqzlPIF9j25N9a7VpQ7gpxNhxQhBFqHFAujSXFcHAYuFAJKJR2rARGXoltgfPz0gV+rVq2irq4WYzRSQCmIOHGygFB62lSxF6MBrAWb9Ytq5erR2Ahsbu7WqdSdZ0M9siIOiVphsMZ/lhMSVw077mQYdGgwIghOL8twHElvb8Bjj/ZiVZSIUM3KlStxHCdOoDgOXpK7j4M+dmZkGz9VSYmkl3/ynwQGRk9x6NDTuG4qpoUKQ2m8RN+JAaSa6AoO1fOqYsbP4ROcOtqHcs89ES+BQn4UTVybKJAoR3Cqd4RiECQlYAlsdcTY2OBp7/H6178BKQXWapQLR4/mGegvJjS02bHQrNVEVfUDZVKov25dn6isGIstjecR6zaefWuNQKkAo80ZYgUxynKVLkrFxaEnT+Yx9swJJ9cX7N3Tz+7dQzj+hMoIp7lGtpxMCk4PihhL2k/jOy6OncrgjEGwbfs9jIx0o6RXplz3dfVO27rIxuA7sPUAUajPuWWbQKCNZnx0mHRFBuk4SBsHWffsOlG+Jxsz2hkfH6a399i092hvn88NN1xPoVBASkWpJNixvTeRJHJ2QsEWZKCJWlpOTGMFy7r6PrQ+CwvAoANFbU2aG26cT3VlijCYID5PmfwtLHPm1CCEJZ/XdB0bRinnWRdPKIctj52ipzuP40wtxBC4rldm74RhhJnRNN9iUI4il6mEGaasQjKcH+TxrbcjlUHgIqRDb1cfJowSjqLBkQ5DfUMc2X0U91lP/1S7ZkINWYy1REHE0EA/Y8UxFl2wGKTET/ns3LqLzh0HYot/IjKuFENDJ06TAH/2Z3+WGIAGpVye2NJFz6lxlDPR72g2nACBNRp/ftuRaQAI2+cf4Qwx9ZkLYIzEdTWXX97C/NYKNl/fSlOTR1g06DBCR5piXtDelqW9LY3AYd+BIQaHgnI28EywkspSKFkeefg4YSiS341NOs/zSKfTiY8cJbl/MalukohZLlU5pSx8sk7QA/Yf+jlP7bgX13NRymG0f4zSeDFpCRNXIB98+hBjQ2OoMmH0uXIhce2wsTGVbMGqNlovnMct738La69chzEhnTue4duf/hrh0GDC/wMTGYRV7OncMiWxJvnYxz7GW9/6KxQLBRyl2LL1JDueHiwzp8qSdRZ0gBEWs2jp/jInEEAuX7qH228/C/0RcOll9cxvraSQz1NT53LjTYt5Zlc/R4+MEYaahsYU6y6tJ+VLBgdDdjzVg1BOuUXamQ+WxXEdjp8osf3Jbi69vAlTUlgiXNchlUqVf7lYKuCn0uUwryUug/YdhcxUMHwG48oBHn/iJziOy6oLNlEYCxkbKlFXmcYYRakQsf/JTlx8tJygYs/gGFmRRDwlEgVCkC/lufINl3LpLZfHrWiEoBSUMNpgIssVm69gKJCMiRxRZDDaIKUlnYbL1l/C6lUXcfPNN3H11VcjhCBf0Gzdepxndo0iXXfWXfM4+pW6Ry9bvMedBoAVF+7Ujnu/svbq5zIDpPAolRzyQZGU7xFFAZ6ruGhdA6vW1KC1wPcFSgqGRkLuu/cYoyMWx1FlWtNMEEwUowgiXFfy85/3M7e1gnlzU+hI4rqqLAFiNVAkikI814vDrZKE3WNIZ6spJEMexEwpg+WRx35EoTDOqhVXMjqYp2F+DcoTHN11nN6j/Tiuj+FMQJ2YPeTiOIowKlLIF1h28WLW3LCWQlhERJPzDDSWjjXLWLFuOZGJCLVFhwatDdY4vPM3LqGiIkNFRTXWWo4cOcJt3/waJ45laWlZh3Q9hAiY9aYRWmPr6vpSl13WPU0COJt/aX9YWzOgRkfBeXYLWMiIrVtPcKBrnFUXVNHeWkc6G9v2nuNgHSiVoOvYCI9vPUXfQBHflWV9fmZwmaRBAighCEPNE4+founmdnI5DyEcstnMtIxcIT+OV52dLNiwBmkNAkV1VR0DA91oo6d0ElBJ3MCydce9HO7ax9JNS1l3zWrGRkrsfWIfUQQqpZHlmMj04LKSHo6j6B88wv0P/5D6hlo+8Je/hVIpRBQg3DhiGfMh4vcwViCkiycMOAYlFUpJxsfh6NGT7N59N/feey933nknR44c5SMf/iLKU9gwTELGs8wJDTXhwrajzhTJOLmuHQsOsu3J5wQAKFzXpa835N57u6iqGqCxLk1FVdxHd3wsoK8/z0B/3Lbd8yzWnjlEOt1zsOXiJ8eRnOge45nOMQb6Huef//mTPPro40xtGlEqFQiDAn4qx0SxiYgrOXDdNNXVTQwMdmPtxMDWSeqXD3T3HeOrn/0/HN77NPnBCFlIk81UxgUjZR5jXAEkJOgoYGj4BHv3PcGevVspBHmGh7v47x/4L6xecxXWhngpH9dLoRyF73ukclnSKSemX6QqELk6TnX3cPsdn+PQoUN0nTjB0OCkIbh8xVXMae0gDJJeJC9BzyAbaZy167ZxJgCYi9du5/HHkiV6LkKIQTkgrcvoWMTw8Eg5Ti+ERQqFUslgRavO3XgVAuUotm/t46mndvLTn943kc9LQj2xITY2NozvpUGpWAJg46kixpJOV1CDZWCgJ84mzviIDLDjye1seXI7aaA6U0ttfQvNDe1UVzcikw5jxgYMDvdy/MQB+nqPUtAhLuAJSVQo8vDP7uLk/mMsXnIJNjJJg0mJEKCExEo3rkeubCHTcSE/ufsL7Hz6nuk+UNJ69rJL3oDEAya6hZ1rk86zMAA9cb9dd9mWMwLAWb/hIf25/3ePsnbz841/nfipkqBkMruOqdlgk/QMFudebZJwCEpRntVrN/HkjqvYvesBrNAJL8wktkCBsbEhKqsb45Yr5eipxRhDNluFRNA/2IO1egYIBD5xAYnGMpIfYOjoAAeP7prRg2TSanEQ+GXSh8Di4BNx7OQespVVLGhfHs8vSurvkAIhPXyrkK6i6+RennnmZ2WJN9Fv0Vpoal7MksWXzMgPzHJlUBRhW+Yd967eeHhq8GoSABuvPMqc1hNE0Vn4wTMaK4mp/YImwsUTdW3nHq+cHNLmce2mt6Fkasp6TG7l2PgQ+fwoSshyh34xAUAD2VwNTY2teK4/w56eqNONa/wU4CZfCoGDwEk23UXgIDFCTKN7TvD9Iiw93YfQWifM40m+jbQRWnmYcIwHf/pFdFQqB7GntrK/csNb8NOZZPycfGkAUCqh163dPjN6Od1GuOySxymG6HKdiDiDCph46Uwg2MnTLl7McIHJzwjDMdra13LF6944Ca4ZI+OGhnswxTxIhbLTm88aY0ilcsxpXkR1RX1smE2yFss1+zZ5HoUgFII+AQMi/n5CGMsp6fKYeRw3j2ltXsjqNVchZEJiLafVYimoHMmunQ8xcOpIXJbFdDeztW0569ZdTykIZjz/bOp/gRH2QXn1pnufEwDOdZvuNUreLw1JO4YXg8LZQLAljEKuu/a9NDUtKi/wdMWmGR7qQetSueXaTJaNVIr6hnnMnbOQXK4GmbSJn+QPxO85LKALSx/Qay1dWHqSvoAiGS0fTxcx1FTUsnrl67jwwivwncq4EGbGvXtuhsMHn2HP/ifIJTHySQBqpFS8/rrfwnEzGCJm/bIxKdWGIbqlpdu99Y1PPTcANl+/J1rQfliEZkr59Ct7aRORzlRx662/h5TumUGpQ4aH+oiiYqx/T1sHizWCTLqGuS0dtLUvpa6mGd/xAEsBy3EsJ6wlsJOUnsBahqzlmLUcspB3PGprW7hw2XrWrtlMY8NCtAZjZ+QohMB3Uxw/voftP78HCaRFPHx66l2tv+yNLF2+niAcfQmmrscSWWKhFGLWb3j4TAGy01/2S1ffZ/71/71belkmmye8gpeFUpBnydL1XH/9B7jjjk+f7pwKidYhA0O9VFXXkUlXnkEKaowFYRS+V8GcxkqC+jaGlKQvKlGVH8cvjBGFRXSSF5ESXD9X7kLa0NhKbTqHHwTo/BgiDHBk0nyJSfKb67gcO7aHJ3fcjzUREoGLxREQ2Yncfwc33PD+OPFlJ6hiZlbFflkKOOJecdMbfnRWABC33Pp989Wv3ykMN5zpNL3sV0IoLRVDfmnz2+nvP8GWLd9LBi3EQxWcRJwFJmRwsAejQyoqGhDCmZzWkQSiJAaMIO+lCSorkekMzcpnrlTJ6Nl47aRSOCoeHuV6KaSImb+RkBgvi5upxJTGMcU8bhgghYkjeErSuXcrz3RuiRNVifsqEGSQFNGkU5X86tv+hHS2hiAoAGqWN5+yt2LCEnbp4v3epk0Hz5TCPu3yVq4aidZf/ijFEufHNUHxCImiiFtv/SDLV1w5GWCyAmXLkhesZWh4gL7+LqIoJn5YYeMhFhYi6TCSrWAkV0nB9bHaYqMQHYVxuXoyE0gpD+m4WAFRWCJKpoFgDVhDpDzCXAO6rp2ofj6iqplCcZSntt3Nrs7HEJjEM4ldUGEtjjUIIXnTmz/C/Pmrk81/CSz+5D0FQBA+aG9+8w+ejcNw5iV/61u/aTH3v9xTQp876hD3H5Iyyzve8T9ZseIKIJ65pxLSlLCTbKF8YZTuU0cZHupHRAJHupR8n4FcDXk/g5YTVNKpU75seeiDtQmp1dpncVWTHETao5RL82TfAb695Yfs7jmAJC4GmdqF3ACegDff8mHWXXwz+bOcUfyi1k2XME0tp9zf/sCPzwkA/g037AlXrfr5TClghSCSTlJD8HJdZpqrqXUBR/n8+ts/zuo1m2PqODaZKzTp4smEeTM4dJKD3XvZMzZIj5vCui6OVChE7LdPcdzOLlApcZRDyvEphSU6O7dw5398hod/9h0GC+P0CskxLN0CCkKWoxoRsOmSG1l/yesplYqz3PzpWWRAIUDfctMPnu3nz9kWVL7917+q/+i/rVFWb7TSQaDwdIE5wz2cqmmh4GZQNkBqEs67KUf+7JSAkEBj5ET1r3kBrejE9DiEMEQ6xFEZfu3XP0ZLzVz23PcltC3XATExWywUkhEsA2EJ3XMINXCSmsp6amqaqayqJZ3K4ahYbwspEz0fj46ZmBkUj4lzUMpBConRIf393RzveoYjh/cwNj44XVlZS0lIAmsYFRYXqHdSXLv+ZpYtXEtp6BSyrv0lCfXGxyXp+GhCotr6H8pf+/VvvCAAuG9965biv395p9y9ZyMZQHo0jA7xtrv+N/3VcznZ1M6hppX01sxlOFNFJF3QcUjWMZpIheVYl5gwrl7UQ9tyQmqi1l4Ij82b38kcE7Ll8e+TL+ZxEkk1jqQfKFpRFvM6LNLXf5y+/uO4bopspoJ0OksuW4/nZ3Bdj1QqTTpdgZAOoYhLugvFPOP5QYYGexke7mZkZGDSuJyWcJ7o5BGzpCILuepm3vDW/8ai9rWUTu6DYglhI7SYkECzqfWT3sPCYseDh/Rbb7g9tbAjekEAAFD/5b2f17//0QuVcTeCxgpJpjRKpmsPrV17uFT+lOGKWnrq2zneuITDzQsYrmxgOFUDxkGZKC7EtGBElARXX2yvs4R0KGI6VlAosnzhWlrq5rB1+z10HnqKfmsZF0kR1ozZgVN5BUPDRYaGe4HD00S86/oJfcpgjEnmE52dqpr6SWvXvp6b3vCbVNQ2EwQhzpxFRCbuOaxekDR8HgBIE3c9CSOihrpTqb/4q399rt9/XgC4b771ydJttz0mt27ZSDqNIMIIB+EmUy6MpWrsFFXD3Sw59DhWZemvaqSrcT5dTcs53DifsUwteScHlHAjm7hEL9ImSFxDaQ02KlEsFslV1rP51o/SOnCMn9z/LcaO7Jhirs0MH82kjk11ncwU65xpnIDpDaTPNC84Bl1j40I2b347ay+6Hm0gKhRA2nhWgXDLpWSz7WQLa0AJbCH/kHn/+7/+fL9/Vq3BxYc++Cn7nt+4WBiziaTMSVgDxo0RLFQyt9wibEj9wFHq+w6yes/9hKkc3bVz6G5YxpHGxZysm8doppKS9FEmHjMXEzniTt6GeGqnFaZs0c8c8TZ1QySWUlRApNPYxg6iynqWzVvOwmWX8/Md9/GzB79L14ndZ/AnzLPYGXDmfjyCyUmklsniajvFSNVUVjaz4cq3sH79zWQztZRKhfiz5BT1JyIkE11JZrvsW2CDAtHiRYdTf/iH354VAHive93x0hvf/D33m7c5IlexsWy9CmbE5RMzXDnxF+CWSrQeP0hr124ulinGcnX01LVxtHkpxxs66K9qIO9VUhIurg5RNsAkmcXYmDHJMokzWgTaWlS2DqeiAe2n4/Kx4hgIl0sufiOrVm5k774n2LrlTvbue4IwzJ8h7yg4vfPIDBAIU47WTbqOk9ecOUtZu3YTa9deQ01tO6UgpFgaP6MNY8um6kvgBVgBYfSE/eBHPnl2EuMcXJHgiiu+7vWdehuuf87RKDGRKTQmHhptDJHvM1wxl2NN7ZxqWML+5gWMZBsoqmzSaqUYM3bNVNE73eAhqTiOJYeesm9JhaBQuG4abMTJ7v3s2/sYe/dt49jRZ2aUZp3jSROSutq5dHSs5sKVG1m46CJSqUrCIIqLV0Q0axm9KNI0N9VwzdUXYZ6raXUKGv/yI2QLPX/jfe3rfzL7ALjttvXqj//0r1XavZqzjAPELVLs5HjYCSKEjQe4YCyYCJSgmMrRX93Kscb5dDUt40RtO2OpSkqOX544Kqw94yRTUR62LCelRXlsS5zqdZSP6ziEUYGR4T56eo5ysruTruMH6B/oolQapVgsJn2PIxAWJR2klDiOR0VFAy0tC2hqWsC8eUtobuogm61C29igtHYmhWR2NPzZAkA4UPG5v/nd+rff8DV31ZqhWQcAQPF3P/hH/o9uv5GK9MaYyGnPGgjJ3iffqESs2oRPIsDq+MtYrOORz1Rxon4+x5sXc6JhASer51H0MkRCIrVF6XjCeDzoOZYIytpyHn6SQmhmxBIUUkkcJVFSYbQhjOJQbxAGGBOidRTTuqSLkBIlHVKpLK7rAQptNJEOsebcOqy+lACw2lQ0iuOmdkVb/uyNxhcQjSpdefWX/RPH32HSqeRkm5dAlyVNJk0E1hL5FYzmajneuJCjzcs4WTePU5WNBI4fj0s3UZwTMDGjxgiSe9PlYtLnKmSdaMhQHv4841ZgSmj4Zb7OFgDG2OzyFSJ/TqrsBQHgnnsXqd/5zc86Sm22Ur30yWKhwcikf3vMlS+kKxipmsfh5iUca17EidpWxtLVBFLFFcVaI22IEQrHJHyA8yCx+UIB0NRYzaZr1s06AF7QhCh/86b9xd/9nc+Kv/+Up3L+Rl7iUTPGOjHXXkkQaYQRpEsB6Z5naDq5k8tUipFsLYO1reybs5Tu+sX0VjUxks6CFURS4xidULpefVfcYMO+JArmBY8IS334o98tde5fxI9/hMrlNk6M1Zj032czuGFBJk2XTCwR4mRUGqHiwEvleD+Voz20H9mKdtMMVtVxsn4hR5qX013fRn+uiYKXAmNRRqNMTOA0SUcvkXw/YaLEz3K+cKImDakzKbKkN3Eaa51zvVvxYnVa4dY3fTL11M8/ZCsySG2w4jxYMGsS78KANBT9WgZqmjjetIgjjUvprZ3DQLaGSLgIq1EmrrxVNg7SaAHCJkUhyTQRgZnGCn45L60NDfWVbLrm4ik2SfmnsRGsxTmL/1kBQHDwgMNvvO8LzsnD7xCpTMJ1f4VFrYhJm8J4GBkijQVtwEagHEZyzfTXzONw80KONS2hr6qJkVQOYRywse2gki6fMvFcTLkZ3fkGAIu1NqukNYuXquLLDgCA0pM/rxUfeP/n3MG+N5NKv/IASKJ7Iqkiskn2ME5Nh4kxGcU5JZWhv6qZ3rpWDsxZTFf9YkYy9Yz7HkpLLCHChnjaRUt9HgLApI0WzvILxNgLVK+zs1nFRx6Zo37nQ591xgdvEV6aqZO4XglW0cQAiFiX61jH23iwhbQTaVPiiJ0J43ZHQhD5WU7WttLTuIDDzUs5WTuf8XQFBeWjbIgwBmnsaQHkmQOtZ/O5nwsA2tjsihcg+mcdAADBAw/Otx/98D/4I6O3Gt+Pp3saphSJvAoua0DHsQcch/F0DSfrOzjRtICjTR30Vs5jLJUjEgKlDcJaXBPE1QpCoAU4Rk6JP8wiADZdPNmEUliMEVnXDYNFi9zovAAAQPjQQ/PMRz/6D97A0C/j+1ipkVbyqrwMSXQyHi1jnCwDVY301M/nWNNijjR1MJJppOCmiYTBjQyO1kTKoOzsyYDTJED8v7QxRi5fIfMv5r3FSxHZKmzfXqs+/KFPOseOv8NmK1BWv+r23iYurcQkFc4SrEboIJYSyiNyU/TWzuVo02K6GhdxvL6NMb+WwJFld1M+S+7ixQLARLpi+QVq7MU+p3ipQpulfQc88wd/8Hepp55aIyoyG84jb/qcDcpyK3mh4r7AMojDzPHkCbAaq3yKmRxd9R2cbFzI0cbF9NTMIe9lCaVC6Qnew4QNMt1OEMwswZn82UwAaGOzvqeDjoVOdN4CoCwNfuu3/tj98R03qkx2g3FkPBsZ+yoDwFTjbuZIqORnNrHOdNzAKvJSjOcaONq4iK6mDo41LmQg10TJ8dHC4ugQVxuMmEIqmZJcM0JMjMpGR5aGhgo2bboYa21amFAuWe7lZ+XpXo7kRuF//+1b3X/7l/cL2Cxcj1epRXDuxqSNIBIgLcVUJQM1jXQ1LOJY03JO1rUylK0mEh4WjaMjpIm7gkhM0k4nocFPAsA3kXWWvQir/xUBAEDxjjtW8Jd//ede1+F5Mpd7FauEc7EjEia0TCKTOkl5K8FYtp5TtW0cbe7gROMiuqvnkXdT6GIIQuEpjU26g04A4OqrLs4uW05+VuXby53ezH/gA//dv+vu65XnbmTK6NbX4mWSxhmT1UomKVdJ0twmTkAYz2fIy3GibfU/HXrrO791suBlBk6N1hdLNoXk81hobKy88drN6+5ZvEQGr2oAABT+/Ssbxf/91O/5PSebRSa3wUhVZtRLO9Fk4jUgIayd0i0kDk9jnbj5NAYrJTKIoDD+ULBy1c+d//oHfyevnizg3PbE4frDh0fmHz063OZ5XvC7H7zsR7Nu4dhXMEWa/8iHf9+9/fYbHRNtEqlcnEnUCmQ8V/O1pia0UEgTxUOzjMEUx9BVNT/U7/mNL6Q//JHvvyImrn2Fc+TFu3+6xH76M7/rPvXUGscVG63nxZ22LK/Nyxoo5gmc1D3mus1388EP/VNq2dLiK3U7rzgAymrhq1/ZIL70ld9wOvcucVy5Ed9/zW28LQZE0rk3uvyyR8T73/v51MarDr/iTq49z1gyhS9+6Wrxja//mrunc5lUbBReGqRKiCZT+/dOqdB5Qa3oZsPKl+UGlCSTTydqJuzEJDEbIfJFgpR/l77s0i28611fTm++du95E+Ww5ylNKv+d710svvntX5E7tq1xC+PXST8Fjleezh03c0va09lXpo1NbNPLpCmVxQiTEFEtNixhwpCopuH7+srX/Uy+7Ve/kbriihPnXZjLnuc8udJjjzXrH37/Vh565HXesa55jrVX25STtHl3Yis76fzxsgNA2KS+wSZBH4solSj5qXvM4o799trr7nbecOOPvCVLgvN1fc97AEwzGL/33Yuie+6+zt/29Frb21vvRcVNuA44ftIY4GWUAsnsXcIiJjJEfuYe2zr3aPS6yx53Nl93l38e6PfXHACmgeHHd6wwjzx2BTu2rXWPHGtjdOwmVxtwBLgOSDXpg08d5Tqz02k55CBmxPCSquKJFjE2KWkLI6y2RK57r6ipHiouXLTfueSSrfbyS7akX3fl0VfbOr5qATDNcNzbmWLnzgvF7t0rins7l2aPdM2Tg/21UaFwkwqjJAuX9O8Vyex65OT4DZG4Z3YyqWMSAn4kFdbz7nKy2XzQ0HAqnN9+JL10+Z7gwmW7s9fdsOfVvnavCQCc6Qr2dqaKJ04WvONdeD3d6L5eSiOjRCOjiHweGZYQOi5LtwKMcjGeD7kMbmUOv7Ia2dhEsaUF3Tq3peKy9d2vxXV6zQLgF9fZXf//AIMW0ueEXYjPAAAAAElFTkSuQmCC".into()
    }
    #[cfg(not(target_os = "macos"))] // 128x128 no padding
    {
        "data:image/png;base64,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".into()
    }
}
