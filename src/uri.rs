use hyper::Uri;

pub trait UriExt {
    fn with_path(&self, path: impl Into<String>) -> Self;
}

impl UriExt for Uri {
    fn with_path(&self, path: impl Into<String>) -> Self {
        let path = path.into();
        let path = if path.starts_with('/') {
            path.replacen('/', "", 1)
        } else {
            path
        };
        format!("{self}{path}").try_into().unwrap()
    }
}
