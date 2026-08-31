const host = process.env.HWMR_URL ?? 'http://127.0.0.1:8787';
const token = process.env.HWMR_TOKEN; if (!token) throw Error('set HWMR_TOKEN after pairing');
const headers = { authorization: `Bearer ${token}` };
const seconds = Number(process.env.HWMR_DURATION_SECONDS ?? 600);
const interval = Number(process.env.HWMR_SAMPLE_INTERVAL_SECONDS ?? process.env.HWMR_SAMPLE_SECONDS ?? 60) * 1000;
const profile = `${process.env.LOCALAPPDATA}\\HWMR\\browser-profile-v7\\DevToolsActivePort`;
const [port] = (await (await import('node:fs/promises')).readFile(profile, 'utf8')).trim().split(/\r?\n/);
const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
const target = targets.find(item => item.type === 'page');

function evaluate(expression) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(target.webSocketDebuggerUrl);
    ws.onopen = () => ws.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate', params: { expression, returnByValue: true } }));
    ws.onmessage = event => { const message = JSON.parse(event.data); if (message.id === 1) { ws.close(); resolve(message.result.result.value); } };
    ws.onerror = reject;
  });
}

async function sample() {
  const [audio, isolation, media] = await Promise.all([
    fetch(`${host}/audio`, { headers }).then(response => response.json()),
    fetch(`${host}/isolation`, { headers }).then(response => response.json()),
    evaluate(`(()=>{const all=[...document.querySelectorAll('video,audio')],i=all.findIndex(m=>!m.paused);const selected=i<0?all[0]:all[i];return {media_count:all.length,selected_media_index:i<0?(all.length?0:-1):i,media:selected&&{tag:selected.tagName.toLowerCase(),paused:selected.paused,currentTime:selected.currentTime,duration:selected.duration,ended:selected.ended,currentSrc:selected.currentSrc,readyState:selected.readyState,networkState:selected.networkState,muted:selected.muted,volume:selected.volume,playbackRate:selected.playbackRate,error:selected.error&&selected.error.code},url:location.href,title:document.title,visibility:document.visibilityState,hidden:document.hidden,timeOrigin:performance.timeOrigin}})()`),
  ]);
  console.log(JSON.stringify({ timestamp: new Date().toISOString(), target_id: target.id, media, audio, isolation }));
}

const until = Date.now() + seconds * 1000;
do { await sample(); if (Date.now() < until) await new Promise(resolve => setTimeout(resolve, interval)); } while (Date.now() < until);
