//! File tree icon mapping — Nerd Font glyphs (primary) with emoji fallback.
//!
//! Returns (icon_str, color) for any file path. Checks filename first (for
//! extensionless files like Dockerfile, Makefile, LICENSE), then extension.
//! The `nerd` flag switches between the two icon sets at runtime.
//!
//! `detect_nerd_font()` probes whether the terminal font renders Nerd Font
//! glyphs by printing a PUA codepoint and measuring cursor advance via DSR.

use ratatui::style::Color;
use std::path::Path;
use std::io::Write;

/// Probe whether the terminal font renders Nerd Font glyphs.
///
/// Prints a known Nerd Font glyph (nf-custom-folder, U+E5FF) at a hidden
/// position, queries cursor column via DSR (Device Status Report), and checks
/// if the glyph advanced the cursor. A proper Nerd Font renders it as 1 column
/// wide. If the font lacks the glyph entirely, some terminals render it as a
/// 2-wide replacement box or a zero-width nothing — either way the column
/// won't be exactly start+1.
///
/// Must be called AFTER entering alternate screen + raw mode (so DSR works).
/// Safe to call during splash — the probe overwrites a cell that gets
/// repainted on the next full draw.
pub fn detect_nerd_font() -> bool {
    let mut stdout = std::io::stdout();
    // Move to bottom-right corner (least visible spot during splash)
    // Row 0 col 0 works too — splash will repaint it
    let _ = crossterm::execute!(
        stdout,
        crossterm::cursor::MoveTo(0, 0),
        crossterm::cursor::SavePosition,
    );

    // Print a Nerd Font glyph (nf-custom-folder U+E5FF) — single-width in patched fonts
    let _ = write!(stdout, "\u{e5ff}");
    let _ = stdout.flush();

    // Query cursor position — crossterm sends DSR and parses response
    // Timeout: crossterm::cursor::position() blocks up to ~1s waiting for terminal response
    let col_after = crossterm::cursor::position().map(|(c, _)| c).unwrap_or(0);

    // Restore cursor and clear the probe glyph
    let _ = crossterm::execute!(
        stdout,
        crossterm::cursor::RestorePosition,
        crossterm::style::Print(" "), // overwrite the probe glyph
        crossterm::cursor::RestorePosition,
    );

    // Nerd Font glyph is 1-wide → cursor should be at column 1 (started at 0)
    // Non-nerd font: typically 0 (zero-width/missing) or 2 (wide replacement box)
    col_after == 1
}

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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Directory icons ──

    #[test]
    fn dir_expanded_nerd() {
        let (icon, color) = file_icon(Path::new("src"), true, true, true);
        assert_eq!(icon, "\u{e5fe} ");
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn dir_collapsed_nerd() {
        let (icon, color) = file_icon(Path::new("src"), true, false, true);
        assert_eq!(icon, "\u{e5ff} ");
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn dir_expanded_emoji() {
        let (icon, color) = file_icon(Path::new("src"), true, true, false);
        assert_eq!(icon, "▼ ");
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn dir_collapsed_emoji() {
        let (icon, color) = file_icon(Path::new("src"), true, false, false);
        assert_eq!(icon, "▶ ");
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn dir_with_dots_in_name() {
        let (icon, _) = file_icon(Path::new("my.dir.name"), true, false, true);
        assert_eq!(icon, "\u{e5ff} ");
    }

    // ── Filename-based matches (nerd mode) ──

    #[test]
    fn nerd_dockerfile() {
        let (icon, color) = file_icon(Path::new("Dockerfile"), false, false, true);
        assert_eq!(icon, "\u{e650} ");
        assert_eq!(color, Color::Rgb(13, 183, 237));
    }

    #[test]
    fn nerd_dockerfile_case_insensitive() {
        let (icon, _) = file_icon(Path::new("DOCKERFILE"), false, false, true);
        assert_eq!(icon, "\u{e650} ");
    }

    #[test]
    fn nerd_containerfile() {
        let (icon, _) = file_icon(Path::new("Containerfile"), false, false, true);
        assert_eq!(icon, "\u{e650} ");
    }

    #[test]
    fn nerd_docker_compose_yml() {
        let (icon, _) = file_icon(Path::new("docker-compose.yml"), false, false, true);
        assert_eq!(icon, "\u{e650} ");
    }

    #[test]
    fn nerd_docker_compose_yaml() {
        let (icon, _) = file_icon(Path::new("docker-compose.yaml"), false, false, true);
        assert_eq!(icon, "\u{e650} ");
    }

    #[test]
    fn nerd_compose_yml() {
        let (icon, _) = file_icon(Path::new("compose.yml"), false, false, true);
        assert_eq!(icon, "\u{e650} ");
    }

    #[test]
    fn nerd_makefile() {
        let (icon, color) = file_icon(Path::new("Makefile"), false, false, true);
        assert_eq!(icon, "\u{e673} ");
        assert_eq!(color, Color::Rgb(224, 148, 64));
    }

    #[test]
    fn nerd_gnumakefile() {
        let (icon, _) = file_icon(Path::new("GNUmakefile"), false, false, true);
        assert_eq!(icon, "\u{e673} ");
    }

    #[test]
    fn nerd_cmakelists() {
        let (icon, color) = file_icon(Path::new("CMakeLists.txt"), false, false, true);
        assert_eq!(icon, "\u{e794} ");
        assert_eq!(color, Color::Rgb(64, 148, 224));
    }

    #[test]
    fn nerd_license() {
        let (icon, color) = file_icon(Path::new("LICENSE"), false, false, true);
        assert_eq!(icon, "\u{e60a} ");
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn nerd_licence_british() {
        let (icon, _) = file_icon(Path::new("LICENCE"), false, false, true);
        assert_eq!(icon, "\u{e60a} ");
    }

    #[test]
    fn nerd_license_md() {
        let (icon, _) = file_icon(Path::new("LICENSE.md"), false, false, true);
        assert_eq!(icon, "\u{e60a} ");
    }

    #[test]
    fn nerd_license_txt() {
        let (icon, _) = file_icon(Path::new("LICENSE.txt"), false, false, true);
        assert_eq!(icon, "\u{e60a} ");
    }

    #[test]
    fn nerd_cargo_toml() {
        let (icon, color) = file_icon(Path::new("Cargo.toml"), false, false, true);
        assert_eq!(icon, "\u{e7a8} ");
        assert_eq!(color, Color::Rgb(222, 165, 132));
    }

    #[test]
    fn nerd_cargo_toml_case_insensitive() {
        let (icon, _) = file_icon(Path::new("CARGO.TOML"), false, false, true);
        assert_eq!(icon, "\u{e7a8} ");
    }

    #[test]
    fn nerd_cargo_lock() {
        let (icon, color) = file_icon(Path::new("Cargo.lock"), false, false, true);
        assert_eq!(icon, "\u{e672} ");
        assert_eq!(color, Color::DarkGray);
    }

    #[test]
    fn nerd_package_json() {
        let (icon, color) = file_icon(Path::new("package.json"), false, false, true);
        assert_eq!(icon, "\u{e616} ");
        assert_eq!(color, Color::Rgb(203, 56, 55));
    }

    #[test]
    fn nerd_package_lock_json() {
        let (icon, color) = file_icon(Path::new("package-lock.json"), false, false, true);
        assert_eq!(icon, "\u{e672} ");
        assert_eq!(color, Color::DarkGray);
    }

    #[test]
    fn nerd_yarn_lock() {
        let (icon, _) = file_icon(Path::new("yarn.lock"), false, false, true);
        assert_eq!(icon, "\u{e672} ");
    }

    #[test]
    fn nerd_pnpm_lock_yaml() {
        let (icon, _) = file_icon(Path::new("pnpm-lock.yaml"), false, false, true);
        assert_eq!(icon, "\u{e672} ");
    }

    #[test]
    fn nerd_gitignore() {
        let (icon, color) = file_icon(Path::new(".gitignore"), false, false, true);
        assert_eq!(icon, "\u{e65d} ");
        assert_eq!(color, Color::Rgb(240, 80, 50));
    }

    #[test]
    fn nerd_gitmodules() {
        let (icon, _) = file_icon(Path::new(".gitmodules"), false, false, true);
        assert_eq!(icon, "\u{e65d} ");
    }

    #[test]
    fn nerd_gitattributes() {
        let (icon, _) = file_icon(Path::new(".gitattributes"), false, false, true);
        assert_eq!(icon, "\u{e65d} ");
    }

    #[test]
    fn nerd_env() {
        let (icon, color) = file_icon(Path::new(".env"), false, false, true);
        assert_eq!(icon, "\u{e615} ");
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn nerd_env_local() {
        let (icon, _) = file_icon(Path::new(".env.local"), false, false, true);
        assert_eq!(icon, "\u{e615} ");
    }

    #[test]
    fn nerd_env_production() {
        let (icon, _) = file_icon(Path::new(".env.production"), false, false, true);
        assert_eq!(icon, "\u{e615} ");
    }

    // ── Extension-based matches (nerd mode) ──

    #[test]
    fn nerd_ext_rs() {
        let (icon, color) = nerd_icon_by_ext(Path::new("main.rs"));
        assert_eq!(icon, "\u{e7a8} ");
        assert_eq!(color, Color::Rgb(222, 165, 132));
    }

    #[test]
    fn nerd_ext_py() {
        let (icon, color) = nerd_icon_by_ext(Path::new("app.py"));
        assert_eq!(icon, "\u{e606} ");
        assert_eq!(color, Color::Rgb(53, 114, 165));
    }

    #[test]
    fn nerd_ext_pyi() {
        let (icon, _) = nerd_icon_by_ext(Path::new("stub.pyi"));
        assert_eq!(icon, "\u{e606} ");
    }

    #[test]
    fn nerd_ext_js() {
        let (icon, color) = nerd_icon_by_ext(Path::new("index.js"));
        assert_eq!(icon, "\u{e60c} ");
        assert_eq!(color, Color::Rgb(247, 223, 30));
    }

    #[test]
    fn nerd_ext_mjs() {
        let (icon, _) = nerd_icon_by_ext(Path::new("module.mjs"));
        assert_eq!(icon, "\u{e60c} ");
    }

    #[test]
    fn nerd_ext_ts() {
        let (icon, color) = nerd_icon_by_ext(Path::new("index.ts"));
        assert_eq!(icon, "\u{e628} ");
        assert_eq!(color, Color::Rgb(49, 120, 198));
    }

    #[test]
    fn nerd_ext_jsx() {
        let (icon, color) = nerd_icon_by_ext(Path::new("App.jsx"));
        assert_eq!(icon, "\u{e625} ");
        assert_eq!(color, Color::Rgb(97, 218, 251));
    }

    #[test]
    fn nerd_ext_tsx() {
        let (icon, color) = nerd_icon_by_ext(Path::new("App.tsx"));
        assert_eq!(icon, "\u{e625} ");
        assert_eq!(color, Color::Rgb(49, 120, 198));
    }

    #[test]
    fn nerd_ext_go() {
        let (icon, color) = nerd_icon_by_ext(Path::new("main.go"));
        assert_eq!(icon, "\u{e627} ");
        assert_eq!(color, Color::Rgb(0, 173, 216));
    }

    #[test]
    fn nerd_ext_java() {
        let (icon, color) = nerd_icon_by_ext(Path::new("Main.java"));
        assert_eq!(icon, "\u{e66d} ");
        assert_eq!(color, Color::Rgb(176, 114, 25));
    }

    #[test]
    fn nerd_ext_c() {
        let (icon, color) = nerd_icon_by_ext(Path::new("main.c"));
        assert_eq!(icon, "\u{e649} ");
        assert_eq!(color, Color::Rgb(85, 141, 200));
    }

    #[test]
    fn nerd_ext_h() {
        let (icon, _) = nerd_icon_by_ext(Path::new("header.h"));
        assert_eq!(icon, "\u{e649} ");
    }

    #[test]
    fn nerd_ext_cpp() {
        let (icon, color) = nerd_icon_by_ext(Path::new("main.cpp"));
        assert_eq!(icon, "\u{e61d} ");
        assert_eq!(color, Color::Rgb(85, 141, 200));
    }

    #[test]
    fn nerd_ext_hpp() {
        let (icon, _) = nerd_icon_by_ext(Path::new("header.hpp"));
        assert_eq!(icon, "\u{e61d} ");
    }

    #[test]
    fn nerd_ext_cs() {
        let (icon, color) = nerd_icon_by_ext(Path::new("Program.cs"));
        assert_eq!(icon, "\u{e648} ");
        assert_eq!(color, Color::Rgb(23, 134, 0));
    }

    #[test]
    fn nerd_ext_swift() {
        let (icon, color) = nerd_icon_by_ext(Path::new("App.swift"));
        assert_eq!(icon, "\u{e699} ");
        assert_eq!(color, Color::Rgb(240, 81, 56));
    }

    #[test]
    fn nerd_ext_rb() {
        let (icon, color) = nerd_icon_by_ext(Path::new("app.rb"));
        assert_eq!(icon, "\u{e605} ");
        assert_eq!(color, Color::Rgb(204, 52, 45));
    }

    #[test]
    fn nerd_ext_php() {
        let (icon, color) = nerd_icon_by_ext(Path::new("index.php"));
        assert_eq!(icon, "\u{e608} ");
        assert_eq!(color, Color::Rgb(79, 93, 149));
    }

    #[test]
    fn nerd_ext_sh() {
        let (icon, color) = nerd_icon_by_ext(Path::new("run.sh"));
        assert_eq!(icon, "\u{e691} ");
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn nerd_ext_bash() {
        let (icon, _) = nerd_icon_by_ext(Path::new("script.bash"));
        assert_eq!(icon, "\u{e691} ");
    }

    #[test]
    fn nerd_ext_html() {
        let (icon, color) = nerd_icon_by_ext(Path::new("index.html"));
        assert_eq!(icon, "\u{e60e} ");
        assert_eq!(color, Color::Rgb(227, 76, 38));
    }

    #[test]
    fn nerd_ext_css() {
        let (icon, color) = nerd_icon_by_ext(Path::new("style.css"));
        assert_eq!(icon, "\u{e614} ");
        assert_eq!(color, Color::Rgb(86, 61, 124));
    }

    #[test]
    fn nerd_ext_scss() {
        let (icon, color) = nerd_icon_by_ext(Path::new("style.scss"));
        assert_eq!(icon, "\u{e603} ");
        assert_eq!(color, Color::Rgb(207, 100, 154));
    }

    #[test]
    fn nerd_ext_vue() {
        let (icon, color) = nerd_icon_by_ext(Path::new("App.vue"));
        assert_eq!(icon, "\u{e6a0} ");
        assert_eq!(color, Color::Rgb(79, 192, 141));
    }

    #[test]
    fn nerd_ext_json() {
        let (icon, color) = nerd_icon_by_ext(Path::new("data.json"));
        assert_eq!(icon, "\u{e60b} ");
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn nerd_ext_yaml() {
        let (icon, color) = nerd_icon_by_ext(Path::new("config.yaml"));
        assert_eq!(icon, "\u{e6a8} ");
        assert_eq!(color, Color::Rgb(203, 56, 55));
    }

    #[test]
    fn nerd_ext_yml() {
        let (icon, _) = nerd_icon_by_ext(Path::new("config.yml"));
        assert_eq!(icon, "\u{e6a8} ");
    }

    #[test]
    fn nerd_ext_toml() {
        let (icon, color) = nerd_icon_by_ext(Path::new("config.toml"));
        assert_eq!(icon, "\u{e6b2} ");
        assert_eq!(color, Color::DarkGray);
    }

    #[test]
    fn nerd_ext_xml() {
        let (icon, color) = nerd_icon_by_ext(Path::new("data.xml"));
        assert_eq!(icon, "\u{e619} ");
        assert_eq!(color, Color::Rgb(224, 148, 64));
    }

    #[test]
    fn nerd_ext_csv() {
        let (icon, color) = nerd_icon_by_ext(Path::new("data.csv"));
        assert_eq!(icon, "\u{e64a} ");
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn nerd_ext_sql() {
        let (icon, color) = nerd_icon_by_ext(Path::new("schema.sql"));
        assert_eq!(icon, "\u{e64d} ");
        assert_eq!(color, Color::Rgb(85, 141, 200));
    }

    #[test]
    fn nerd_ext_md() {
        let (icon, color) = nerd_icon_by_ext(Path::new("README.md"));
        assert_eq!(icon, "\u{e609} ");
        assert_eq!(color, Color::Rgb(83, 141, 213));
    }

    #[test]
    fn nerd_ext_txt() {
        let (icon, color) = nerd_icon_by_ext(Path::new("notes.txt"));
        assert_eq!(icon, "\u{e64e} ");
        assert_eq!(color, Color::White);
    }

    #[test]
    fn nerd_ext_pdf() {
        let (icon, color) = nerd_icon_by_ext(Path::new("doc.pdf"));
        assert_eq!(icon, "\u{e67d} ");
        assert_eq!(color, Color::Rgb(210, 46, 46));
    }

    #[test]
    fn nerd_ext_png() {
        let (icon, color) = nerd_icon_by_ext(Path::new("image.png"));
        assert_eq!(icon, "\u{e60d} ");
        assert_eq!(color, Color::Rgb(160, 100, 200));
    }

    #[test]
    fn nerd_ext_svg() {
        let (icon, color) = nerd_icon_by_ext(Path::new("logo.svg"));
        assert_eq!(icon, "\u{e698} ");
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn nerd_ext_mp3() {
        let (icon, color) = nerd_icon_by_ext(Path::new("song.mp3"));
        assert_eq!(icon, "\u{e638} ");
        assert_eq!(color, Color::Rgb(160, 100, 200));
    }

    #[test]
    fn nerd_ext_mp4() {
        let (icon, color) = nerd_icon_by_ext(Path::new("video.mp4"));
        assert_eq!(icon, "\u{e69f} ");
        assert_eq!(color, Color::Rgb(224, 148, 64));
    }

    #[test]
    fn nerd_ext_zip() {
        let (icon, color) = nerd_icon_by_ext(Path::new("archive.zip"));
        assert_eq!(icon, "\u{e6aa} ");
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn nerd_ext_lock() {
        let (icon, color) = nerd_icon_by_ext(Path::new("Gemfile.lock"));
        assert_eq!(icon, "\u{e672} ");
        assert_eq!(color, Color::DarkGray);
    }

    #[test]
    fn nerd_ext_log() {
        let (icon, color) = nerd_icon_by_ext(Path::new("app.log"));
        assert_eq!(icon, "\u{e68f} ");
        assert_eq!(color, Color::DarkGray);
    }

    #[test]
    fn nerd_ext_wasm() {
        let (icon, color) = nerd_icon_by_ext(Path::new("module.wasm"));
        assert_eq!(icon, "\u{e6a1} ");
        assert_eq!(color, Color::Rgb(101, 79, 240));
    }

    #[test]
    fn nerd_ext_db() {
        let (icon, color) = nerd_icon_by_ext(Path::new("data.db"));
        assert_eq!(icon, "\u{e64d} ");
        assert_eq!(color, Color::Rgb(85, 141, 200));
    }

    #[test]
    fn nerd_ext_sqlite() {
        let (icon, _) = nerd_icon_by_ext(Path::new("data.sqlite"));
        assert_eq!(icon, "\u{e64d} ");
    }

    #[test]
    fn nerd_ext_pem() {
        let (icon, color) = nerd_icon_by_ext(Path::new("cert.pem"));
        assert_eq!(icon, "\u{f084} ");
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn nerd_ext_key() {
        let (icon, _) = nerd_icon_by_ext(Path::new("private.key"));
        assert_eq!(icon, "\u{f084} ");
    }

    // ── Emoji fallback mode ──

    #[test]
    fn emoji_ext_rs() {
        let (icon, color) = emoji_icon_by_ext(Path::new("main.rs"));
        assert_eq!(icon, "🦀");
        assert_eq!(color, Color::White);
    }

    #[test]
    fn emoji_ext_toml() {
        let (icon, _) = emoji_icon_by_ext(Path::new("config.toml"));
        assert_eq!(icon, "⚙ ");
    }

    #[test]
    fn emoji_ext_md() {
        let (icon, _) = emoji_icon_by_ext(Path::new("README.md"));
        assert_eq!(icon, "📝");
    }

    #[test]
    fn emoji_ext_json() {
        let (icon, _) = emoji_icon_by_ext(Path::new("data.json"));
        assert_eq!(icon, "{}");
    }

    #[test]
    fn emoji_ext_yaml() {
        let (icon, _) = emoji_icon_by_ext(Path::new("config.yaml"));
        assert_eq!(icon, "📋");
    }

    #[test]
    fn emoji_ext_yml() {
        let (icon, _) = emoji_icon_by_ext(Path::new("config.yml"));
        assert_eq!(icon, "📋");
    }

    #[test]
    fn emoji_ext_lock() {
        let (icon, _) = emoji_icon_by_ext(Path::new("Cargo.lock"));
        assert_eq!(icon, "🔒");
    }

    #[test]
    fn emoji_ext_unknown() {
        let (icon, color) = emoji_icon_by_ext(Path::new("file.xyz"));
        assert_eq!(icon, "  ");
        assert_eq!(color, Color::White);
    }

    // ── Unknown / default extensions ──

    #[test]
    fn nerd_unknown_ext_returns_default() {
        let (icon, color) = nerd_icon_by_ext(Path::new("file.xyzabc"));
        assert_eq!(icon, "\u{e64e} ");
        assert_eq!(color, Color::White);
    }

    #[test]
    fn nerd_no_extension_returns_default() {
        let (icon, color) = nerd_icon_by_ext(Path::new("noextension"));
        assert_eq!(icon, "\u{e64e} ");
        assert_eq!(color, Color::White);
    }

    // ── Paths with directories ──

    #[test]
    fn nerd_nested_path_rs() {
        let (icon, _) = file_icon(Path::new("src/app/main.rs"), false, false, true);
        assert_eq!(icon, "\u{e7a8} ");
    }

    #[test]
    fn nerd_nested_dockerfile() {
        let (icon, _) = file_icon(Path::new("docker/Dockerfile"), false, false, true);
        assert_eq!(icon, "\u{e650} ");
    }

    // ── file_icon integrating nerd ext via the public API ──

    #[test]
    fn file_icon_nerd_rs_file() {
        let (icon, color) = file_icon(Path::new("lib.rs"), false, false, true);
        assert_eq!(icon, "\u{e7a8} ");
        assert_eq!(color, Color::Rgb(222, 165, 132));
    }

    #[test]
    fn file_icon_emoji_fallback_file() {
        let (icon, _) = file_icon(Path::new("lib.rs"), false, false, false);
        assert_eq!(icon, "🦀");
    }

    // ── Additional nerd extension coverage ──

    #[test]
    fn nerd_ext_lua() {
        let (icon, color) = nerd_icon_by_ext(Path::new("init.lua"));
        assert_eq!(icon, "\u{e620} ");
        assert_eq!(color, Color::Rgb(0, 0, 128));
    }

    #[test]
    fn nerd_ext_zig() {
        let (icon, _) = nerd_icon_by_ext(Path::new("main.zig"));
        assert_eq!(icon, "\u{e6a9} ");
    }

    #[test]
    fn nerd_ext_kt() {
        let (icon, color) = nerd_icon_by_ext(Path::new("Main.kt"));
        assert_eq!(icon, "\u{e634} ");
        assert_eq!(color, Color::Rgb(169, 123, 255));
    }

    #[test]
    fn nerd_ext_ex() {
        let (icon, _) = nerd_icon_by_ext(Path::new("app.ex"));
        assert_eq!(icon, "\u{e62d} ");
    }

    #[test]
    fn nerd_ext_hs() {
        let (icon, _) = nerd_icon_by_ext(Path::new("Main.hs"));
        assert_eq!(icon, "\u{e61f} ");
    }

    #[test]
    fn nerd_ext_scala() {
        let (icon, _) = nerd_icon_by_ext(Path::new("App.scala"));
        assert_eq!(icon, "\u{e68e} ");
    }

    #[test]
    fn nerd_ext_r() {
        let (icon, _) = nerd_icon_by_ext(Path::new("analysis.r"));
        assert_eq!(icon, "\u{e68a} ");
    }

    #[test]
    fn nerd_ext_svelte() {
        let (icon, _) = nerd_icon_by_ext(Path::new("App.svelte"));
        assert_eq!(icon, "\u{e697} ");
    }

    #[test]
    fn nerd_ext_graphql() {
        let (icon, _) = nerd_icon_by_ext(Path::new("schema.graphql"));
        assert_eq!(icon, "\u{e662} ");
    }

    #[test]
    fn nerd_ext_tar() {
        let (icon, _) = nerd_icon_by_ext(Path::new("archive.tar"));
        assert_eq!(icon, "\u{e6aa} ");
    }

    #[test]
    fn nerd_ext_gz() {
        let (icon, _) = nerd_icon_by_ext(Path::new("data.gz"));
        assert_eq!(icon, "\u{e6aa} ");
    }

    #[test]
    fn nerd_ext_exe() {
        let (icon, color) = nerd_icon_by_ext(Path::new("prog.exe"));
        assert_eq!(icon, "\u{eae8} ");
        assert_eq!(color, Color::DarkGray);
    }

    #[test]
    fn nerd_ext_dll() {
        let (icon, _) = nerd_icon_by_ext(Path::new("lib.dll"));
        assert_eq!(icon, "\u{eae8} ");
    }

    #[test]
    fn nerd_ext_ttf() {
        let (icon, color) = nerd_icon_by_ext(Path::new("font.ttf"));
        assert_eq!(icon, "\u{e659} ");
        assert_eq!(color, Color::Rgb(210, 46, 46));
    }

    #[test]
    fn nerd_ext_woff2() {
        let (icon, _) = nerd_icon_by_ext(Path::new("font.woff2"));
        assert_eq!(icon, "\u{e659} ");
    }

    #[test]
    fn nerd_ext_jpg() {
        let (icon, _) = nerd_icon_by_ext(Path::new("photo.jpg"));
        assert_eq!(icon, "\u{e60d} ");
    }

    #[test]
    fn nerd_ext_gif() {
        let (icon, _) = nerd_icon_by_ext(Path::new("anim.gif"));
        assert_eq!(icon, "\u{e60d} ");
    }

    #[test]
    fn nerd_ext_wav() {
        let (icon, _) = nerd_icon_by_ext(Path::new("audio.wav"));
        assert_eq!(icon, "\u{e638} ");
    }

    #[test]
    fn nerd_ext_mkv() {
        let (icon, _) = nerd_icon_by_ext(Path::new("movie.mkv"));
        assert_eq!(icon, "\u{e69f} ");
    }

    #[test]
    fn nerd_ext_ico() {
        let (icon, color) = nerd_icon_by_ext(Path::new("favicon.ico"));
        assert_eq!(icon, "\u{e623} ");
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn nerd_ext_less() {
        let (icon, _) = nerd_icon_by_ext(Path::new("vars.less"));
        assert_eq!(icon, "\u{e614} ");
    }

    #[test]
    fn nerd_ext_ini() {
        let (icon, color) = nerd_icon_by_ext(Path::new("config.ini"));
        assert_eq!(icon, "\u{e615} ");
        assert_eq!(color, Color::DarkGray);
    }

    #[test]
    fn nerd_ext_tex() {
        let (icon, color) = nerd_icon_by_ext(Path::new("paper.tex"));
        assert_eq!(icon, "\u{e69b} ");
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn nerd_ext_ps1() {
        let (icon, _) = nerd_icon_by_ext(Path::new("script.ps1"));
        assert_eq!(icon, "\u{e683} ");
    }

    #[test]
    fn nerd_ext_zsh() {
        let (icon, _) = nerd_icon_by_ext(Path::new("profile.zsh"));
        assert_eq!(icon, "\u{e691} ");
    }

    #[test]
    fn nerd_ext_fish() {
        let (icon, _) = nerd_icon_by_ext(Path::new("config.fish"));
        assert_eq!(icon, "\u{e691} ");
    }

    // ── Edge case: dir flag overrides filename match ──

    #[test]
    fn dir_flag_overrides_rs_extension() {
        // Even if the path looks like a file, is_dir=true should give directory icon
        let (icon, _) = file_icon(Path::new("main.rs"), true, false, true);
        assert_eq!(icon, "\u{e5ff} ");
    }

    #[test]
    fn dir_flag_overrides_dockerfile_name() {
        let (icon, _) = file_icon(Path::new("Dockerfile"), true, true, true);
        assert_eq!(icon, "\u{e5fe} ");
    }

    // ── Emoji mode doesn't crash on special filenames ──

    #[test]
    fn emoji_mode_dockerfile() {
        let (icon, color) = file_icon(Path::new("Dockerfile"), false, false, false);
        // No special emoji for Dockerfile — falls through to default
        assert_eq!(icon, "  ");
        assert_eq!(color, Color::White);
    }

    #[test]
    fn emoji_mode_no_extension() {
        let (icon, color) = file_icon(Path::new("README"), false, false, false);
        assert_eq!(icon, "  ");
        assert_eq!(color, Color::White);
    }

    // ── Unicode / special character filenames ──

    #[test]
    fn unicode_filename_with_rs_ext() {
        let (icon, _) = file_icon(Path::new("модуль.rs"), false, false, true);
        assert_eq!(icon, "\u{e7a8} ");
    }

    #[test]
    fn dot_hidden_file_nerd() {
        // A dot-file without matching filename falls to extension check
        let (icon, _) = file_icon(Path::new(".hidden"), false, false, true);
        // No extension, not a known filename -> default
        assert_eq!(icon, "\u{e64e} ");
    }

    #[test]
    fn nerd_ext_nix() {
        let (icon, _) = nerd_icon_by_ext(Path::new("default.nix"));
        assert_eq!(icon, "\u{f1105} ");
    }

    #[test]
    fn nerd_ext_tf() {
        let (icon, color) = nerd_icon_by_ext(Path::new("main.tf"));
        assert_eq!(icon, "\u{e69a} ");
        assert_eq!(color, Color::Rgb(92, 78, 229));
    }

    #[test]
    fn nerd_ext_proto() {
        let (icon, _) = nerd_icon_by_ext(Path::new("service.proto"));
        assert_eq!(icon, "\u{e615} ");
    }

    #[test]
    fn nerd_ext_rst() {
        let (icon, _) = nerd_icon_by_ext(Path::new("docs.rst"));
        assert_eq!(icon, "\u{e64e} ");
    }
}
