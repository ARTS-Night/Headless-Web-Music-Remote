const host = 'http://127.0.0.1:8787', token = process.env.HWMR_TOKEN;
if (!token) throw Error('set HWMR_TOKEN after pairing');
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
const control = message => new Promise((resolve, reject) => { const ws = new WebSocket(`${host.replace('http', 'ws')}/ws/control`); ws.onopen = () => { ws.send(JSON.stringify({ type: 'auth', token })); ws.send(JSON.stringify(message)); }; ws.onmessage = event => { ws.close(); resolve(JSON.parse(event.data)); }; ws.onerror = reject; });
const active = async () => (await (await fetch(`${host}/tabs`, { headers: { authorization: `Bearer ${token}` } })).json()).items.find(tab => tab.active);
const expect = (value, message) => { if (!value) throw Error(message); };
for (const [text, expected] of [['https://example.com/?hwmr=one', 'example.com'], ['https://example.org/?hwmr=two', 'example.org']]) { const result = await control({ type: 'go', text }); expect(result.ok, result.error); await wait(1500); expect((await active()).url.includes(expected), `${expected} navigation failed`); }
for (const [type, expected] of [['back', 'example.com'], ['forward', 'example.org'], ['reload', 'example.org']]) { const result = await control({ type }); expect(result.ok, `${type}: ${result.error}`); await wait(1200); expect((await active()).url.includes(expected), `${type} failed`); }
console.log('PASS navigation back/forward/reload');
