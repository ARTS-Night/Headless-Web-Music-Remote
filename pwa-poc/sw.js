const CACHE = 'hwmr-pwa-shell-v2';
const SHELL = ['./', './index.html', './manifest.webmanifest', './icons/hwmr.svg'];
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
  event.respondWith(caches.match(event.request).then(cached => cached || fetch(event.request)));
});
