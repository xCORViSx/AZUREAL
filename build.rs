fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("resources/Azureal.ico");
        res.compile().expect("failed to compile Windows resources");
    }
}
