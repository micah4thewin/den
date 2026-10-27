"use strict";

// The shelf's own pages, kept so the app opens on a phone that has wandered
// out of range. The library itself is never cached: what is on the machine
// right now is the only answer worth giving, so /api/ always goes to the wire
// and an unreachable machine is reported as unreachable.

const SHELF = "play-shelf-v1";
const SHELL = ["/", "/app.js", "/style.css", "/icon-192.png", "/manifest.webmanifest"];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(SHELF)
      .then((cache) => cache.addAll(SHELL))
      .then(() => self.skipWaiting())
      .catch(() => self.skipWaiting())
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((names) => Promise.all(names.filter((name) => name !== SHELF).map((name) => caches.delete(name))))
      .then(() => self.clients.claim())
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;

  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  if (url.pathname.startsWith("/api/")) return;

  // The network first, always, so a phone never runs last week's app against
  // this week's shelf. The cache is the answer only when there is no network.
  event.respondWith(
    fetch(request)
      .then((response) => {
        if (response && response.ok) {
          const keep = response.clone();
          caches.open(SHELF).then((cache) => cache.put(request, keep));
        }
        return response;
      })
      .catch(() =>
        caches.match(request).then((hit) => {
          if (hit) return hit;
          if (request.mode === "navigate") return caches.match("/");
          return Response.error();
        })
      )
  );
});
