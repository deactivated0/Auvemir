# A calmer native listening room

Auvemir uses native Slint components and a shared semantic palette: canvas, sidebar, surface, raised surface, hover, border, primary text, muted text, accent, accent surface and text on accent. Dark blue-green surfaces and soft green accents give the collection a distinct identity. Light appearance uses warm off-white, deep green text and a stronger green accent. Platform fonts avoid a bundled font dependency.

A single sidebar destination replaces duplicate entries. Scrollable playlists cannot push Queue or Settings off the window. Phones keep navigation and playback controls, with Ambient and Recordings accessible from Home. Split book controls separate online discovery from local imports. Settings uses seven task-based sections. Import progress, cancellation, persistent sorting and useful empty/loading states explain what is happening.

Screen headings reserve their space, keeping navigation free of subtitle layout shifts. Secondary text uses a short replacement delay, 240 ms opacity transition and 270 ms upward movement of 5 logical pixels with ease-out. Hover and press feedback use 130 ms native color animation. There is no permanent animation loop. A native one-shot timer handles text replacement and stops after use. Rapid navigation replaces the pending subtitle with the latest value.

Reduced motion disables the transitions. Native settings are read once at startup: GNOME/KDE, Windows client-area animation, macOS Reduce Motion, Android animator scale, and iOS accessibility. The explicit in-app reduced-motion setting applies immediately and persists. The desktop system query is bounded and child processes are reaped. Live OS preference changes require relaunch; this is documented rather than implied to update live.

Keyboard focus, readable contrast, button labels/default actions, keyboard-operable sliders/rows and a shortcut guide improve access. Device screen-reader and dynamic-type certification are still required. The Linux viewport/DPI suite checks layout rendering, not a full assistive-technology audit.

Category references: [Amberol](https://apps.gnome.org/Amberol/) for a focused adaptive local player and [Strawberry](https://www.strawberrymusicplayer.org/) for a capable personal collection. These inform restraint and information organization, without copying assets or claiming endorsement.
