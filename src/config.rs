pub struct Configuration {
    pub work_dir: String,
    pub app_data_root_hash: u64,
}

impl Configuration {
    pub fn new(dir: String, root_hash: u64) -> Configuration {
        Configuration {
            work_dir: dir,
            app_data_root_hash: root_hash,
        }
    }
}
