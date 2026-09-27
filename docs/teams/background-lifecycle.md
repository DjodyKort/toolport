# Required Teams lifecycle

GTK's application timer and Tauri's native Rust worker own Teams synchronization. Hiding the window or launching into the tray does not pause configuration, membership checks, policy receipts, or required success counters. Exiting the application stops this work.

A connected client long-polls configuration for up to 25 seconds, then waits 3 seconds. A configuration change wakes the poll. Successful calls are reported on the next completed cycle (normally within 28 seconds plus request latency). Network failures retry after 15, 30, then at most 60 seconds. These are scheduling bounds, not guarantees during network or server outages. Manual and background sync serialize within the process; a result from a superseded connection cannot overwrite the new connection's config.

Required counters live in the atomic, locked per-device activity journal. The server acknowledges a cumulative revision. A call arriving during an upload remains unacknowledged, retries do not double count, and offline history has no today/yesterday cutoff. Config application and operational receipts do not require optional call export. Legacy daily savings and optional per-call export retain their separate limitations.

Synthetic validation: GTK hidden after close-to-tray and Tauri launched with `--hidden` both received configuration, reported the applied version, and reported a successful real gateway call without showing the window. Tauri also retained a call during a backend outage and reported it after reconnection. No production accounts or content were used.
