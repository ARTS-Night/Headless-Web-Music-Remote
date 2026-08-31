use anyhow::Result;
use windows::{
    Win32::{
        Media::Audio::{
            AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2,
            IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender,
        },
        System::Com::{
            CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        },
    },
    core::Interface,
};

fn main() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let endpoint = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let manager: IAudioSessionManager2 = endpoint.Activate(CLSCTX_ALL, None)?;
        let sessions = manager.GetSessionEnumerator()?;
        for index in 0..sessions.GetCount()? {
            let session = sessions.GetSession(index)?;
            let control: IAudioSessionControl2 = session.cast()?;
            println!(
                "pid={} active={}",
                control.GetProcessId()?,
                session.GetState()? == AudioSessionStateActive
            );
        }
        CoUninitialize();
    }
    Ok(())
}
