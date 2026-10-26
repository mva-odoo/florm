// Windows : incorpore l'icône dans florm.exe (Explorateur, raccourcis, barre des tâches).
fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/windows/florm.ico");
        res.compile().expect("icône Windows");
    }
}
