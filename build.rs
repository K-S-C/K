fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("src/k-logo.ico");
        res.compile().unwrap();
    }
}
