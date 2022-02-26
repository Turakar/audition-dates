use std::collections::HashMap;

use accept_language::intersection as accept_language_intersection;
use fluent_templates::{static_loader, FluentLoader, StaticLoader};
use rocket::{
    request::{FromRequest, Outcome},
    Request,
};

pub const SUPPORTED_LANGUAGES: [&str; 2] = ["de", "en"];

static_loader! {
    pub static LOCALES = {
        locales: "./locales",
        fallback_language: "de",
    };
}

pub struct Language {
    language: String,
}

impl Language {
    pub fn into_string(self) -> String {
        self.language
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Language {
    type Error = std::convert::Infallible;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        Outcome::Success(Language {
            language: match req.cookies().get("language") {
                Some(cookie) if is_supported_language(cookie.value()) => {
                    String::from(cookie.value())
                }
                _ => match req.headers().get_one("accept-language") {
                    Some(value) => {
                        let common_languages =
                            accept_language_intersection(value, SUPPORTED_LANGUAGES.to_vec());
                        match common_languages.into_iter().next() {
                            Some(language) => language,
                            None => "en".into(),
                        }
                    }
                    None => "de".into(),
                },
            },
        })
    }
}

pub fn make_fluent_loader() -> FluentLoader<&'static StaticLoader> {
    FluentLoader::new(&*LOCALES)
}

fn is_supported_language(lang: &str) -> bool {
    SUPPORTED_LANGUAGES
        .into_iter()
        .any(|supported| supported == lang)
}

pub fn supported_languages(_args: &HashMap<String, tera::Value>) -> tera::Result<tera::Value> {
    Ok(tera::to_value(SUPPORTED_LANGUAGES)?)
}
