const CACHE = 'hwmr-pwa-shell-v6';
const SHELL = ['./', './index.html', './manifest.webmanifest', './icons/hwmr.svg', './jsQR.min.js'];
self.addEventListener('install', event =>
  event.waitUntil(
    caches.open(CACHE).then(cache => cache.addAll(SHELL)).then(() => self.skipWaiting())
  )
);
self.addEventListener('activate', event =>
  event.waitUntil(
    caches.keys()
      .then(keys => Promise.all(keys.filter(key => key !== CACHE).map(key => caches.delete(key))))
      .then(() => self.clients.claim())
  )
);
self.addEventListener('fetch', event => {
  const url = new URL(event.request.url);
  // Only cache Pages-origin (static shell). LAN host traffic is cross-origin and must not be cached.
  if (url.origin !== self.location.origin) return;
  const documentRequest = event.request.mode === 'navigate'
    || url.pathname.endsWith('/')
    || url.pathname.endsWith('/index.html');
  if (documentRequest) {
    event.respondWith(
      fetch(event.request, { cache: 'no-store' })
        .then(response => {
          if (response.ok) {
            return caches.open(CACHE).then(cache => {
              cache.put(event.request, response.clone());
              return response;
            });
          }
          return response;
        })
        .catch(() => caches.match(event.request))
    );
  } else {
    event.respondWith(caches.match(event.request).then(cached => cached || fetch(event.request)));
  }
});
