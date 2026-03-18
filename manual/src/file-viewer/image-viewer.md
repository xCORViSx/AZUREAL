# Image Viewer

When you open an image file in the Viewer, AZUREAL renders it directly in the
terminal using a graphics protocol. This lets you preview images without leaving
the application.

---

## Supported Formats

| Format | Extensions |
|--------|-----------|
| PNG | `.png` |
| JPEG | `.jpg`, `.jpeg` |
| GIF | `.gif` |
| BMP | `.bmp` |
| WebP | `.webp` |
| ICO | `.ico` |

Files with these extensions are detected as images and routed to the image
viewer automatically. Other binary files are not rendered.

---

## Terminal Graphics Protocols

Image rendering uses the **ratatui-image** crate, which supports three terminal
graphics protocols in order of preference:

### Kitty Graphics Protocol

The Kitty protocol transmits image data as base64-encoded payloads directly to
the terminal. It supports full-color rendering at the terminal's native
resolution and is the highest-quality option. Terminals that support this
protocol include Kitty, WezTerm, and Ghostty.

### Sixel

Sixel is an older graphics protocol supported by a wide range of terminals
(mlterm, foot, xterm with Sixel enabled, and others). It encodes images as
six-pixel-tall horizontal bands. Color depth varies by terminal, but modern
implementations typically support 256 or more colors.

### Halfblock Fallback

If neither Kitty nor Sixel is available, the image viewer falls back to
**halfblock rendering**. This uses the Unicode half-block character to encode
two vertical pixels per terminal cell, with foreground and background colors
representing the top and bottom pixel respectively. The result is lower
resolution than native graphics protocols but works in any terminal that
supports 24-bit color.

### Protocol Detection

The graphics protocol is **auto-detected once** on the first image load. The
detection probes the terminal's capabilities and selects the best available
protocol. The result is cached for the remainder of the session -- subsequent
image loads skip detection and use the cached protocol immediately.

---

## Viewport Behavior

Images are **auto-fitted** to the Viewer viewport. The image is scaled to fill
the available space while preserving its aspect ratio. If the image is smaller
than the viewport, it is displayed at its native size (no upscaling).

Unlike source files, image view has no scrolling, no text selection, and no
cursor. The image is a static render that fills the viewport.

---

## Edit Mode

Edit mode is not available for image files. Pressing `e` on an image has no
effect. Images are view-only.
