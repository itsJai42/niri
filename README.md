<h1 align="center"><img alt="niri-next" src="./assets/niri-next.png"></h1>
<p align="center">A scrollable-tiling Wayland compositor — community-driven, feature-forward.</p>
<p align="center">
    <!-- <a href="https://matrix.to/#/#niri-next:matrix.org"><img alt="Matrix" src="https://img.shields.io/badge/matrix-%23niri--next-blue?logo=matrix"></a> -->
    <a href="https://github.com/itsJai42/niri-next/blob/main/LICENSE"><img alt="GitHub License" src="https://img.shields.io/github/license/itsJai42/niri-next"></a>
    <a href="https://github.com/itsJai42/niri-next/releases"><img alt="GitHub Release" src="https://img.shields.io/github/v/release/itsJai42/niri-next?logo=github"></a>
</p>

<p align="center">
    <a href="https://niri.github.io/niri/Getting-Started.html">Getting Started</a> | <a href="https://niri.github.io/niri/Configuration%3A-Introduction.html">Configuration</a> | <a href="https://github.com/niri/niri/discussions/325">Setup&nbsp;Showcase</a>
</p>

<img width="1280" height="720" alt="niri with a few windows open" src="https://github.com/user-attachments/assets/dea5909e-1859-4aaa-9d88-d37f9663e00b" />

## About

Niri-next is a community-driven fork of [niri](https://github.com/niri-wm/niri) that tracks upstream while moving faster on features. We merge upstream regularly, accept pull requests with a lighter touch, and ship rolling improvements alongside stable releases.

Windows are arranged in columns on an infinite strip going to the right.
Opening a new window never causes existing windows to resize.

Every monitor has its own separate window strip.
Windows can never "overflow" onto an adjacent monitor.

Workspaces are dynamic and arranged vertically.
Every monitor has an independent set of workspaces, and there's always one empty workspace present all the way down.

The workspace arrangement is preserved across disconnecting and connecting monitors where it makes sense.
When a monitor disconnects, its workspaces will move to another monitor, but upon reconnection they will move back to the original monitor.

## Features

- Built from the ground up for scrollable tiling
- [Dynamic workspaces](https://niri.github.io/niri/Workspaces.html) like in GNOME
- An [Overview](https://github.com/user-attachments/assets/379a5d1f-acdb-4c11-b36c-e85fd91f0995) that zooms out workspaces and windows
- Built-in screenshot UI
- Monitor and window screencasting through xdg-desktop-portal-gnome
    - You can [block out](https://niri.github.io/niri/Configuration%3A-Window-Rules.html#block-out-from) sensitive windows from screencasts
    - [Dynamic cast target](https://niri.github.io/niri/Screencasting.html#dynamic-screencast-target) that can change what it shows on the go
- [Touchpad](https://github.com/niri-wm/niri/assets/1794388/946a910e-9bec-4cd1-a923-4a9421707515) and [mouse](https://github.com/niri-wm/niri/assets/1794388/8464e65d-4bf2-44fa-8c8e-5883355bd000) gestures
- Group windows into [tabs](https://niri.github.io/niri/Tabs.html)
- Configurable layout: gaps, borders, struts, window sizes
- [Gradient borders](https://niri.github.io/niri/Configuration%3A-Layout.html#gradients) with Oklab and Oklch support
- [Background blur](https://niri.github.io/niri/Window-Effects.html) for windows and layer-shell surfaces
- [Animations](https://github.com/niri-wm/niri/assets/1794388/ce178da2-af9e-4c51-876f-8703c241d95e) with support for [custom shaders](https://github.com/niri-wm/niri/assets/1794388/27a238d6-0a22-4692-b794-30dc7a626fad)
- Live-reloading config
- Works with [screen readers](https://niri.github.io/niri-next/Accessibility.html)
- **Community-driven**: lighter PR review process — good ideas get merged faster
- **Rolling features**: improvements land as they're ready, not gated behind major releases
- **Upstream tracking**: regular merges from [niri](https://github.com/niri-wm/niri) keep the base stable and current

## Video Demo

https://github.com/niri-wm/niri/assets/1794388/bce834b0-f205-434e-a027-b373495f9729

Also check out these videos that showcase a lot of the niri/niri-next functionality:

- [Niri Is My New Favorite Wayland Compositor](https://www.youtube.com/watch?v=DeYx2exm04M) by Brodie Robertson
- [How Is niri This Good? Live Demo + Config](https://www.youtube.com/watch?v=7XmD5UyyhZQ) by Nick Janetakis

## Status

Niri-next is stable for day-to-day use and does most things expected of a Wayland compositor.

Give it a try!
Follow the instructions on the [Getting Started](https://niri.github.io/niri/Getting-Started.html) page.
Grab a desktop shell like [DankMaterialShell] or [Noctalia] (or build a more traditional setup): niri-next by itself is not a complete desktop environment.
Also check out [awesome-niri], a list of niri-related links and projects.

Here are some points you may ask about:

- **Multi-monitor**: yes, a core part of the design from the very start. Mixed DPI works.
- **Fractional scaling**: yes, plus all niri-next UI stays pixel-perfect.
- **NVIDIA**: seems to work fine.
- **Floating windows**: yes, starting from niri 25.01.
- **Input devices**: niri-next supports tablets, touchpads, and touchscreens.
You can map the tablet to a specific monitor, or use [OpenTabletDriver].
We have touchpad gestures, but no touchscreen gestures yet.
- **Wlr protocols**: yes, we have most of the important ones like layer-shell, gamma-control, screencopy.
You can check on [wayland.app](https://wayland.app) at the bottom of each protocol's page.
- **Performance**: while we run niri-next on beefy machines, we try to stay conscious of performance.
I've seen someone use it fine on an Eee PC 900 from 2008, of all things.
- **Xwayland**: [integrated](https://niri-next.github.io/niri-next/Xwayland.html#using-xwayland-satellite) via xwayland-satellite.
- **Relationship to niri**: niri-next is a friendly fork. we track upstream, contribute back where it makes sense, and focus on the features and pace the community wants. no drama, just velocity.

## Media

[niri: Making a Wayland compositor in Rust](https://youtu.be/Kmz8ODolnDg?list=PLRdS-n5seLRqrmWDQY4KDqtRMfIwU0U3T) · *December 2024*

Talk from the 2024 Moscow RustCon about niri, and how randomized property testing and profiling work, and measuring input latency.
The talk is in Russian, with full English subtitles in YouTube's subtitle language selector.

[An interview with Ivan, the developer behind Niri](https://www.trommelspeicher.de/podcast/special_the_developer_behind_niri) · *June 2025*

An interview by a German tech podcast Das Triumvirat (in English) about niri development and history.

[A tour of the niri scrolling-tiling Wayland compositor](https://lwn.net/Articles/1025866/) · *July 2025*

An LWN article with a nice overview and introduction to niri.

## Contributing

Niri-next exists because the community wanted a faster-moving option — and we mean it.
If you'd like to help, there are plenty of both coding- and non-coding-related ways to do so, and our PR process is lighter than upstream.
See [CONTRIBUTING.md](https://github.com/niri-next/niri-next/blob/main/CONTRIBUTING.md) for the overview.

## Inspiration

Niri-next is heavily inspired by [PaperWM] which implements scrollable tiling on top of GNOME Shell, and builds on the excellent work of [niri](https://github.com/niri-wm/niri).

One of the reasons that prompted the original niri project was being able to properly separate the monitors.
Being a GNOME Shell extension, PaperWM has to work against Shell's global window coordinate space to prevent windows from overflowing.

## Tile Scrollably Elsewhere

Here are some other projects which implement a similar workflow:

- [PaperWM]: scrollable tiling on top of GNOME Shell.
- [karousel]: scrollable tiling on top of KDE.
- [scroll](https://github.com/dawsers/scroll) and [papersway]: scrollable tiling on top of sway/i3.
- Hyprland has a built-in [scrolling layout](https://wiki.hypr.land/Configuring/Layouts/Scrolling-Layout/).
- [Paneru] and [PaperWM.spoon]: scrollable tiling on top of macOS.

## Useful Projects
- [PaperWM](https://github.com/paperwm/PaperWM)
- [waybar](https://github.com/Alexays/Waybar)
- [fuzzel](https://codeberg.org/dnkl/fuzzel)
- [awesome-niri](https://github.com/niri-wm/awesome-niri)
- [karousel](https://github.com/peterfajdiga/karousel)
- [papersway](https://spwhitton.name/tech/code/papersway/)
- [Paneru](https://github.com/karinushka/paneru)
- [PaperWM.spoon](https://github.com/mogenson/PaperWM.spoon)
- [Matrix channel](https://matrix.to/#/#niri:matrix.org)
- [OpenTabletDriver](https://opentabletdriver.net/)
- [DankMaterialShell](https://danklinux.com/)
- [Noctalia](https://noctalia.dev/)


