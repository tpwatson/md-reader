use std::path::PathBuf;

const APP_NAME: &str = "MD Reader";
const PROGID: &str = "MDReader.Markdown";
const EXTENSIONS: &[&str] = &[".md", ".markdown", ".mdown", ".mkd"];

pub fn register_and_prompt() -> Result<String, String> {
    if cfg!(debug_assertions) {
        return Err(
            "This is a development build. Install MD Reader, open the installed app, and use this button there."
                .into(),
        );
    }
    register_current_exe()?;
    prompt_default_ui()?;
    Ok("Windows opened the default-app list. Check the Markdown extensions and save.".into())
}

#[cfg(windows)]
pub fn register_current_exe() -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let exe = current_executable()?;
    let exe_name = exe
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Could not read the program file name.")?;
    let command = format!("\"{}\" \"%1\"", exe.display());
    let icon = format!("\"{}\",0", exe.display());
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    let prog = create(&hkcu, &format!(r"Software\Classes\{PROGID}"))?;
    set_default(&prog, "Markdown Document")?;

    let prog_icon = create(&hkcu, &format!(r"Software\Classes\{PROGID}\DefaultIcon"))?;
    set_default(&prog_icon, &icon)?;

    let prog_command = create(
        &hkcu,
        &format!(r"Software\Classes\{PROGID}\shell\open\command"),
    )?;
    set_default(&prog_command, &command)?;

    let capabilities = create(&hkcu, r"Software\MD Reader\Capabilities")?;
    capabilities
        .set_value("ApplicationName", &APP_NAME)
        .map_err(|error| error.to_string())?;
    capabilities
        .set_value("ApplicationDescription", &"Read Markdown files")
        .map_err(|error| error.to_string())?;

    let associations = create(
        &hkcu,
        r"Software\MD Reader\Capabilities\FileAssociations",
    )?;
    for extension in EXTENSIONS {
        associations
            .set_value(extension, &PROGID)
            .map_err(|error| format!("Could not register {extension}: {error}"))?;

        let open_with = create(
            &hkcu,
            &format!(r"Software\Classes\{extension}\OpenWithProgids"),
        )?;
        open_with
            .set_value(PROGID, &"")
            .map_err(|error| format!("Could not add {extension} to Open with: {error}"))?;
    }

    let registered = create(&hkcu, r"Software\RegisteredApplications")?;
    registered
        .set_value(APP_NAME, &r"Software\MD Reader\Capabilities")
        .map_err(|error| error.to_string())?;

    let application = create(
        &hkcu,
        &format!(r"Software\Classes\Applications\{exe_name}"),
    )?;
    application
        .set_value("FriendlyAppName", &APP_NAME)
        .map_err(|error| error.to_string())?;
    let app_command = create(
        &hkcu,
        &format!(r"Software\Classes\Applications\{exe_name}\shell\open\command"),
    )?;
    set_default(&app_command, &command)?;
    let supported = create(
        &hkcu,
        &format!(r"Software\Classes\Applications\{exe_name}\SupportedTypes"),
    )?;
    for extension in EXTENSIONS {
        supported
            .set_value(extension, &"")
            .map_err(|error| error.to_string())?;
    }

    notify_shell();
    Ok(())
}

#[cfg(not(windows))]
pub fn register_current_exe() -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
fn prompt_default_ui() -> Result<(), String> {
    use std::ffi::c_void;
    use windows::core::{GUID, HRESULT, IUnknown, Interface, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };

    #[repr(C)]
    struct Vtbl {
        query_interface: unsafe extern "system" fn(
            this: *mut c_void,
            iid: *const GUID,
            out: *mut *mut c_void,
        ) -> HRESULT,
        add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
        release: unsafe extern "system" fn(this: *mut c_void) -> u32,
        launch: unsafe extern "system" fn(this: *mut c_void, name: PCWSTR) -> HRESULT,
    }

    const CLSID: GUID = GUID::from_u128(0x1968106d_f3b5_44cf_890e_116fcb9ecef1);
    const IID: GUID = GUID::from_u128(0x1f76a169_f994_40ac_8fc8_0959e8874710);

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let unknown: IUnknown = CoCreateInstance(&CLSID, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| format!("Could not open Windows default apps: {error}"))?;
        let this = unknown.as_raw();
        let vtbl = *(this as *const *const Vtbl);
        let mut iface = std::ptr::null_mut();
        ((*vtbl).query_interface)(this, &IID, &mut iface)
            .ok()
            .map_err(|error| format!("Could not open Windows default apps: {error}"))?;
        if iface.is_null() {
            return Err("Could not open Windows default apps.".into());
        }

        let iface_vtbl = *(iface as *const *const Vtbl);
        let name: Vec<u16> = "MD Reader"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let result = ((*iface_vtbl).launch)(iface, PCWSTR(name.as_ptr()));
        ((*iface_vtbl).release)(iface);
        result
            .ok()
            .map_err(|error| format!("Could not open Windows default apps: {error}"))?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn prompt_default_ui() -> Result<(), String> {
    Err("Set MD Reader as the default app for Markdown files in your system settings.".into())
}

#[cfg(windows)]
fn current_executable() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|error| format!("Could not locate this program: {error}"))?;
    let exe = crate::markdown::plain_path(&exe);
    if !exe.is_file() {
        return Err("Could not locate this program.".into());
    }
    Ok(exe)
}

#[cfg(windows)]
fn create(root: &winreg::RegKey, path: &str) -> Result<winreg::RegKey, String> {
    root.create_subkey(path)
        .map(|(key, _)| key)
        .map_err(|error| format!("Could not write registry key {path}: {error}"))
}

#[cfg(windows)]
fn set_default(key: &winreg::RegKey, value: &str) -> Result<(), String> {
    key.set_value("", &value).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn notify_shell() {
    const SHCNE_ASSOCCHANGED: u32 = 0x0800_0000;
    #[link(name = "shell32")]
    extern "system" {
        fn SHChangeNotify(
            event_id: u32,
            flags: u32,
            item1: *const core::ffi::c_void,
            item2: *const core::ffi::c_void,
        );
    }
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, 0, std::ptr::null(), std::ptr::null());
    }
}
