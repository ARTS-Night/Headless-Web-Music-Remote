#[path = "../audio.rs"]
mod audio;

fn main() -> anyhow::Result<()> {
    for session in audio::sessions()? {
        println!("pid={} active={}", session.pid, session.active);
    }
    Ok(())
}
