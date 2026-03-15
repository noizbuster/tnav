#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserOpenOutcome {
    pub url: String,
    pub opened: bool,
    pub failure: Option<String>,
}

pub trait BrowserOpener {
    fn open(&self, url: &str) -> BrowserOpenOutcome;
}

#[derive(Debug, Clone, Default)]
pub struct WebbrowserBrowser;

impl WebbrowserBrowser {
    pub fn new() -> Self {
        Self
    }
}

impl BrowserOpener for WebbrowserBrowser {
    fn open(&self, url: &str) -> BrowserOpenOutcome {
        match webbrowser::open(url) {
            Ok(()) => BrowserOpenOutcome {
                url: url.to_owned(),
                opened: true,
                failure: None,
            },
            Err(error) => BrowserOpenOutcome {
                url: url.to_owned(),
                opened: false,
                failure: Some(error.to_string()),
            },
        }
    }
}
