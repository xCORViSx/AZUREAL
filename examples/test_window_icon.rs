// Test: set the Windows Terminal window icon (title bar / taskbar preview).
// Tries multiple approaches to find what works in modern WT.

#[cfg(windows)]
fn main() {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    // Win32 constants
    const WM_SETICON: u32 = 0x0080;
    const ICON_SMALL: usize = 0;
    const ICON_BIG: usize = 1;
    const IMAGE_ICON: u32 = 1;
    const LR_LOADFROMFILE: u32 = 0x0010;
    const LR_DEFAULTSIZE: u32 = 0x0040;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetConsoleWindow() -> *mut std::ffi::c_void;
    }

    #[link(name = "user32")]
    extern "system" {
        fn SendMessageW(
            hwnd: *mut std::ffi::c_void,
            msg: u32,
            wparam: usize,
            lparam: isize,
        ) -> isize;
        fn LoadImageW(
            hinst: *mut std::ffi::c_void,
            name: *const u16,
            img_type: u32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> *mut std::ffi::c_void;
        fn FindWindowW(
            class: *const u16,
            title: *const u16,
        ) -> *mut std::ffi::c_void;
        fn GetWindowThreadProcessId(
            hwnd: *mut std::ffi::c_void,
            process_id: *mut u32,
        ) -> u32;
    }

    // Callback for EnumWindows
    type EnumWindowsProc = unsafe extern "system" fn(*mut std::ffi::c_void, isize) -> i32;
    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(callback: EnumWindowsProc, lparam: isize) -> i32;
        fn GetClassNameW(hwnd: *mut std::ffi::c_void, buf: *mut u16, max: i32) -> i32;
        fn IsWindowVisible(hwnd: *mut std::ffi::c_void) -> i32;
    }

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    let ico_path = format!(
        "{}/.azureal/Azureal.ico",
        std::env::var("USERPROFILE").unwrap()
    );
    let ico_wide = to_wide(&ico_path);

    // Load icon handles
    let hicon_big = unsafe {
        LoadImageW(
            std::ptr::null_mut(),
            ico_wide.as_ptr(),
            IMAGE_ICON,
            64, 64,
            LR_LOADFROMFILE,
        )
    };
    let hicon_small = unsafe {
        LoadImageW(
            std::ptr::null_mut(),
            ico_wide.as_ptr(),
            IMAGE_ICON,
            16, 16,
            LR_LOADFROMFILE,
        )
    };
    println!("hicon_big:   {:?} (null={})", hicon_big, hicon_big.is_null());
    println!("hicon_small: {:?} (null={})", hicon_small, hicon_small.is_null());

    if hicon_big.is_null() && hicon_small.is_null() {
        eprintln!("ERROR: Could not load icon from {}", ico_path);
        std::process::exit(1);
    }

    // --- Approach 1: GetConsoleWindow ---
    let hwnd_console = unsafe { GetConsoleWindow() };
    println!("\n=== Approach 1: GetConsoleWindow ===");
    println!("hwnd: {:?} (null={})", hwnd_console, hwnd_console.is_null());
    if !hwnd_console.is_null() {
        unsafe {
            if !hicon_big.is_null() {
                SendMessageW(hwnd_console, WM_SETICON, ICON_BIG, hicon_big as isize);
                println!("Sent WM_SETICON ICON_BIG");
            }
            if !hicon_small.is_null() {
                SendMessageW(hwnd_console, WM_SETICON, ICON_SMALL, hicon_small as isize);
                println!("Sent WM_SETICON ICON_SMALL");
            }
        }
    }

    // --- Approach 2: Find CASCADIA_HOSTING_WINDOW_CLASS (WT's window class) ---
    println!("\n=== Approach 2: Find WT window by class ===");
    let my_pid = std::process::id();
    println!("My PID: {}", my_pid);

    // WT window class name
    let wt_class = to_wide("CASCADIA_HOSTING_WINDOW_CLASS");

    // Use FindWindow with WT class
    let hwnd_wt = unsafe { FindWindowW(wt_class.as_ptr(), std::ptr::null()) };
    println!("FindWindow(CASCADIA_HOSTING_WINDOW_CLASS): {:?} (null={})", hwnd_wt, hwnd_wt.is_null());

    if !hwnd_wt.is_null() {
        unsafe {
            if !hicon_big.is_null() {
                SendMessageW(hwnd_wt, WM_SETICON, ICON_BIG, hicon_big as isize);
                println!("Sent WM_SETICON ICON_BIG to WT window");
            }
            if !hicon_small.is_null() {
                SendMessageW(hwnd_wt, WM_SETICON, ICON_SMALL, hicon_small as isize);
                println!("Sent WM_SETICON ICON_SMALL to WT window");
            }
        }
    }

    // --- Approach 3: EnumWindows to find all WT windows ---
    println!("\n=== Approach 3: EnumWindows ===");

    static mut FOUND_HWNDS: Vec<(*mut std::ffi::c_void, String)> = Vec::new();

    unsafe extern "system" fn enum_callback(hwnd: *mut std::ffi::c_void, _lparam: isize) -> i32 {
        if IsWindowVisible(hwnd) == 0 {
            return 1; // continue
        }
        let mut class_buf = [0u16; 256];
        let len = GetClassNameW(hwnd, class_buf.as_mut_ptr(), 256);
        if len > 0 {
            let class_name = String::from_utf16_lossy(&class_buf[..len as usize]);
            if class_name.contains("CASCADIA") || class_name.contains("Console") {
                FOUND_HWNDS.push((hwnd, class_name));
            }
        }
        1 // continue
    }

    unsafe {
        FOUND_HWNDS.clear();
        EnumWindows(enum_callback, 0);
        for (hwnd, class) in &FOUND_HWNDS {
            let mut pid = 0u32;
            GetWindowThreadProcessId(*hwnd, &mut pid);
            println!("  hwnd={:?} class={:?} pid={}", hwnd, class, pid);

            if !hicon_big.is_null() {
                SendMessageW(*hwnd, WM_SETICON, ICON_BIG, hicon_big as isize);
            }
            if !hicon_small.is_null() {
                SendMessageW(*hwnd, WM_SETICON, ICON_SMALL, hicon_small as isize);
            }
            println!("    -> Sent WM_SETICON to this window");
        }
    }

    println!("\nDone! Check the title bar and taskbar preview.");
    println!("Press Enter to exit...");
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf).ok();
}

#[cfg(not(windows))]
fn main() {
    println!("This test is Windows-only");
}
