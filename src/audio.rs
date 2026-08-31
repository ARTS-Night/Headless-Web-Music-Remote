use anyhow::Result;
use serde::Serialize;
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

#[derive(Serialize)]
pub struct AudioSession {
    pub pid: u32,
    pub active: bool,
}

pub fn sessions() -> Result<Vec<AudioSession>> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let endpoint = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let manager: IAudioSessionManager2 = endpoint.Activate(CLSCTX_ALL, None)?;
        let enumerator = manager.GetSessionEnumerator()?;
        let result = (0..enumerator.GetCount()?)
            .map(|index| {
                let session = enumerator.GetSession(index)?;
                let control: IAudioSessionControl2 = session.cast()?;
                Ok(AudioSession {
                    pid: control.GetProcessId()?,
                    active: session.GetState()? == AudioSessionStateActive,
                })
            })
            .collect();
        CoUninitialize();
        result
    }
}
