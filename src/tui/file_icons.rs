//! File tree icon mapping — Nerd Font glyphs (primary) with emoji fallback.
//!
//! Returns (icon_str, color) for any file path. Checks filename first (for
//! extensionless files like Dockerfile, Makefile, LICENSE), then extension.
//! The `nerd` flag switches between the two icon sets at runtime.

use ratatui::style::Color;
use std::path::Path;

/// Icon result: the glyph string (with trailing space for alignment) and its color.
/// Nerd Font glyphs are single-width in patched fonts, so "X " gives icon+space.
/// Emoji are double-width, so they already occupy 2 columns — no trailing space.
pub fn file_icon(path: &Path, is_dir: bool, expanded: bool, nerd: bool) -> (&'static str, Color) {
    if is_dir {
        return if nerd {
            // nf-custom-folder_open / nf-custom-folder
            if expanded { ("\u{e5fe} ", Color::Yellow) } else { ("\u{e5ff} ", Color::Yellow) }
        } else {
            if expanded { ("▼ ", Color::Yellow) } else { ("▶ ", Color::Yellow) }
        };
    }

    // Check full filename first (for extensionless files and special names)
    let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
    let filename_lower = filename.to_ascii_lowercase();

    if nerd {
        // Filename-based matches (Nerd Font)
        match filename_lower.as_str() {
            "dockerfile" | "containerfile" => return ("\u{e650} ", Color::Rgb(13, 183, 237)),  // docker blue
            "docker-compose.yml" | "docker-compose.yaml" | "compose.yml" | "compose.yaml"
                => return ("\u{e650} ", Color::Rgb(13, 183, 237)),
            "makefile" | "gnumakefile" => return ("\u{e673} ", Color::Rgb(224, 148, 64)),       // orange
            "cmakelists.txt" => return ("\u{e794} ", Color::Rgb(64, 148, 224)),                 // cmake blue
            "license" | "licence" | "license.md" | "licence.md" | "license.txt" | "licence.txt"
                => return ("\u{e60a} ", Color::Yellow),
            "cargo.toml" => return ("\u{e7a8} ", Color::Rgb(222, 165, 132)),                    // rust orange
            "cargo.lock" => return ("\u{e672} ", Color::DarkGray),
            "package.json" => return ("\u{e616} ", Color::Rgb(203, 56, 55)),                    // npm red
            "package-lock.json" => return ("\u{e672} ", Color::DarkGray),
            "yarn.lock" => return ("\u{e672} ", Color::DarkGray),
            "pnpm-lock.yaml" => return ("\u{e672} ", Color::DarkGray),
            ".gitignore" | ".gitmodules" | ".gitattributes" | ".gitkeep"
                => return ("\u{e65d} ", Color::Rgb(240, 80, 50)),                               // git orange
            ".env" | ".env.local" | ".env.development" | ".env.production"
                => return ("\u{e615} ", Color::Yellow),
            _ => {}
        }

        // Extension-based matches (Nerd Font)
        nerd_icon_by_ext(path)
    } else {
        // Emoji fallback — same sparse set as the original implementation
        emoji_icon_by_ext(path)
    }
}

/// Nerd Font icons by file extension — comprehensive coverage with language-brand colors
fn nerd_icon_by_ext(path: &Path) -> (&'static str, Color) {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext.to_ascii_lowercase().as_str() {
        // ── Programming languages ──
        "rs"         => ("\u{e7a8} ", Color::Rgb(222, 165, 132)),   // rust orange
        "py" | "pyi" | "pyw"
                     => ("\u{e606} ", Color::Rgb(53, 114, 165)),    // python blue
        "js" | "mjs" | "cjs"
                     => ("\u{e60c} ", Color::Rgb(247, 223, 30)),    // js yellow
        "ts" | "mts" | "cts"
                     => ("\u{e628} ", Color::Rgb(49, 120, 198)),    // ts blue
        "jsx"        => ("\u{e625} ", Color::Rgb(97, 218, 251)),    // react cyan
        "tsx"        => ("\u{e625} ", Color::Rgb(49, 120, 198)),    // react + ts blue
        "go"         => ("\u{e627} ", Color::Rgb(0, 173, 216)),     // go cyan
        "java"       => ("\u{e66d} ", Color::Rgb(176, 114, 25)),    // java brown
        "c" | "h"    => ("\u{e649} ", Color::Rgb(85, 141, 200)),    // c blue
        "cpp" | "cc" | "cxx" | "hpp" | "hxx"
                     => ("\u{e61d} ", Color::Rgb(85, 141, 200)),    // c++ blue
        "cs"         => ("\u{e648} ", Color::Rgb(23, 134, 0)),      // c# green
        "swift"      => ("\u{e699} ", Color::Rgb(240, 81, 56)),     // swift orange
        "kt" | "kts" => ("\u{e634} ", Color::Rgb(169, 123, 255)),   // kotlin purple
        "rb"         => ("\u{e605} ", Color::Rgb(204, 52, 45)),     // ruby red
        "php"        => ("\u{e608} ", Color::Rgb(79, 93, 149)),     // php purple
        "lua"        => ("\u{e620} ", Color::Rgb(0, 0, 128)),       // lua navy
        "zig"        => ("\u{e6a9} ", Color::Rgb(236, 145, 92)),    // zig orange
        "ex" | "exs" => ("\u{e62d} ", Color::Rgb(110, 74, 126)),    // elixir purple
        "hs" | "lhs" => ("\u{e61f} ", Color::Rgb(93, 79, 133)),     // haskell purple
        "scala" | "sc"
                     => ("\u{e68e} ", Color::Rgb(220, 50, 47)),     // scala red
        "r"          => ("\u{e68a} ", Color::Rgb(39, 109, 195)),    // r blue
        "pl" | "pm"  => ("\u{e67e} ", Color::Rgb(57, 69, 124)),     // perl blue
        "sh" | "bash" | "zsh" | "fish"
                     => ("\u{e691} ", Color::Green),                 // shell green
        "ps1" | "psm1" | "psd1"
                     => ("\u{e683} ", Color::Rgb(1, 36, 86)),       // powershell blue

        // ── Web / markup ──
        "html" | "htm"
                     => ("\u{e60e} ", Color::Rgb(227, 76, 38)),     // html orange
        "css"        => ("\u{e614} ", Color::Rgb(86, 61, 124)),     // css purple
        "scss" | "sass"
                     => ("\u{e603} ", Color::Rgb(207, 100, 154)),   // sass pink
        "less"       => ("\u{e614} ", Color::Rgb(29, 54, 93)),      // less dark blue
        "vue"        => ("\u{e6a0} ", Color::Rgb(79, 192, 141)),    // vue green
        "svelte"     => ("\u{e697} ", Color::Rgb(255, 62, 0)),      // svelte orange
        "astro"      => ("\u{e697} ", Color::Rgb(255, 90, 3)),      // astro orange

        // ── Data / config ──
        "json" | "jsonc" | "json5"
                     => ("\u{e60b} ", Color::Yellow),
        "yaml" | "yml"
                     => ("\u{e6a8} ", Color::Rgb(203, 56, 55)),     // yaml red
        "toml"       => ("\u{e6b2} ", Color::DarkGray),
        "xml" | "xsl" | "xslt"
                     => ("\u{e619} ", Color::Rgb(224, 148, 64)),    // xml orange
        "csv"        => ("\u{e64a} ", Color::Green),
        "sql"        => ("\u{e64d} ", Color::Rgb(85, 141, 200)),    // sql blue
        "graphql" | "gql"
                     => ("\u{e662} ", Color::Rgb(225, 0, 152)),     // graphql pink
        "proto"      => ("\u{e615} ", Color::LightBlue),
        "ini" | "cfg" | "conf"
                     => ("\u{e615} ", Color::DarkGray),

        // ── Documentation ──
        "md" | "mdx" => ("\u{e609} ", Color::Rgb(83, 141, 213)),    // markdown blue
        "txt"        => ("\u{e64e} ", Color::White),
        "pdf"        => ("\u{e67d} ", Color::Rgb(210, 46, 46)),     // pdf red
        "tex" | "latex"
                     => ("\u{e69b} ", Color::Green),
        "rst"        => ("\u{e64e} ", Color::DarkGray),

        // ── DevOps ──
        "tf" | "tfvars"
                     => ("\u{e69a} ", Color::Rgb(92, 78, 229)),     // terraform purple
        "nix"        => ("\u{f1105} ", Color::Rgb(126, 186, 228)),  // nix blue

        // ── Media: images ──
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "tiff" | "tif"
                     => ("\u{e60d} ", Color::Rgb(160, 100, 200)),   // image purple
        "svg"        => ("\u{e698} ", Color::Yellow),
        "ico"        => ("\u{e623} ", Color::Yellow),

        // ── Media: audio ──
        "mp3" | "wav" | "flac" | "ogg" | "aac" | "m4a" | "wma"
                     => ("\u{e638} ", Color::Rgb(160, 100, 200)),   // audio purple

        // ── Media: video ──
        "mp4" | "avi" | "mkv" | "mov" | "webm" | "wmv" | "flv"
                     => ("\u{e69f} ", Color::Rgb(224, 148, 64)),    // video orange

        // ── Media: fonts ──
        "ttf" | "otf" | "woff" | "woff2" | "eot"
                     => ("\u{e659} ", Color::Rgb(210, 46, 46)),     // font red

        // ── Archives ──
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "zst"
                     => ("\u{e6aa} ", Color::Yellow),

        // ── Lock files ──
        "lock"       => ("\u{e672} ", Color::DarkGray),

        // ── Logs ──
        "log"        => ("\u{e68f} ", Color::DarkGray),

        // ── Certificates / keys ──
        "pem" | "crt" | "cer" | "key" | "p12" | "pfx"
                     => ("\u{f084} ", Color::Yellow),

        // ── Databases ──
        "db" | "sqlite" | "sqlite3"
                     => ("\u{e64d} ", Color::Rgb(85, 141, 200)),

        // ── Binaries / executables ──
        "wasm"       => ("\u{e6a1} ", Color::Rgb(101, 79, 240)),    // wasm purple
        "o" | "so" | "dylib" | "dll" | "exe"
                     => ("\u{eae8} ", Color::DarkGray),              // binary

        // ── Default: generic file ──
        _            => ("\u{e64e} ", Color::White),
    }
}

/// Emoji fallback icons — the original sparse set for terminals without Nerd Fonts
fn emoji_icon_by_ext(path: &Path) -> (&'static str, Color) {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => ("🦀", Color::White),
        Some("toml") => ("⚙ ", Color::White),
        Some("md") => ("📝", Color::White),
        Some("json") => ("{}", Color::White),
        Some("yaml") | Some("yml") => ("📋", Color::White),
        Some("lock") => ("🔒", Color::White),
        _ => ("  ", Color::White),
    }
}
