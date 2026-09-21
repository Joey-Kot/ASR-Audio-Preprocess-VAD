use smartaudio::Cancellation;
use std::sync::OnceLock;
static TOKEN: OnceLock<Cancellation> = OnceLock::new();

#[cfg(unix)]
extern "C" fn interrupt(_: libc::c_int) {
    if let Some(token) = TOKEN.get() {
        token.cancel();
    }
}
#[cfg(windows)]
unsafe extern "system" fn interrupt(kind: u32) -> i32 {
    if kind <= 1 {
        if let Some(token) = TOKEN.get() {
            token.cancel();
        }
        1
    } else {
        0
    }
}
pub fn install(token: Cancellation) -> smartaudio::Result<()> {
    TOKEN
        .set(token)
        .map_err(|_| smartaudio::Error::Conversion("signal handler already installed".into()))?;
    #[cfg(unix)]
    unsafe {
        if libc::signal(libc::SIGINT, interrupt as *const () as libc::sighandler_t) == libc::SIG_ERR
            || libc::signal(libc::SIGTERM, interrupt as *const () as libc::sighandler_t)
                == libc::SIG_ERR
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(windows)]
    unsafe {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn SetConsoleCtrlHandler(
                handler: Option<unsafe extern "system" fn(u32) -> i32>,
                add: i32,
            ) -> i32;
        }
        if SetConsoleCtrlHandler(Some(interrupt), 1) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}
