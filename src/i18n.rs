use fluent::concurrent::FluentBundle;
use fluent::{FluentArgs, FluentResource, FluentValue};
use fluent_langneg::{negotiate_languages, LanguageIdentifier, NegotiationStrategy};
use once_cell::sync::Lazy;
use rust_embed::RustEmbed;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// # Examples
/// ```rust
/// use jenkins::i18n::I18n;
/// use jenkins::i18n::t;
///
/// I18n::set_locale("zh-CN"); // Optional, set locale(default is system locale)
/// println!("current locale: {}", I18n::locale());
/// println!("available locales: {:?}", I18n::available_locales());
/// println!("{}", t!("hello-world"));
/// println!("{}", t!("welcome", "name" => "张三")); // with args
/// println!("{}", t!("welcome", "name" => "Zhang San"; "en-US")); // Optional, get translation with specified locale
/// ```

/// Embed all localization resource files
#[derive(RustEmbed)]
#[folder = "locales/"]
struct LocaleAssets;

type ConcurrentFluentBundle = FluentBundle<FluentResource>;

static BUNDLES: Lazy<RwLock<HashMap<String, Arc<ConcurrentFluentBundle>>>> = Lazy::new(|| RwLock::new(load_bundles()));
fn load_bundles() -> HashMap<String, Arc<ConcurrentFluentBundle>> {
    let mut bundles = HashMap::new();
    for file in LocaleAssets::iter() {
        if let Some(content) = LocaleAssets::get(&file) {
            let lang = file.as_ref().split('.').next().unwrap().to_string();
            let resource =
                FluentResource::try_new(std::str::from_utf8(content.data.as_ref()).unwrap().to_owned()).unwrap();
            let mut bundle = ConcurrentFluentBundle::new_concurrent(vec![lang.parse().unwrap()]);
            bundle.add_resource(resource).unwrap();
            bundles.insert(lang, Arc::new(bundle));
        }
    }
    bundles
}

pub const DEFAULT_LOCALE: &str = "en-US";
/// Get the system locale
pub fn get_system_locale() -> String {
    sys_locale::get_locale()
        .map(|locale| normalize_locale(&locale))
        .unwrap_or_else(|| DEFAULT_LOCALE.to_string())
}

/// Normalize the locale string
/// - Returns the default locale "en-US" if input is "C" or "POSIX"
/// - Replaces underscores with hyphens, e.g., "zh_CN" => "zh-CN"
fn normalize_locale(locale: &str) -> String {
    match locale {
        "C" | "POSIX" => DEFAULT_LOCALE.to_string(),
        _ => locale.replace('_', "-"),
    }
}

static CURRENT_LOCALE: Lazy<RwLock<String>> = Lazy::new(|| RwLock::new(get_system_locale()));

pub struct I18n;

impl I18n {
    #[allow(dead_code)]
    pub fn set_locale(locale: &str) {
        let normalized_locale = normalize_locale(locale);
        let mut current_locale = CURRENT_LOCALE.write().unwrap();
        *current_locale = normalized_locale;
    }

    #[allow(dead_code)]
    pub fn locale() -> String {
        CURRENT_LOCALE.read().unwrap().clone()
    }

    #[allow(dead_code)]
    pub fn available_locales() -> Vec<String> {
        let bundles = BUNDLES.read().unwrap();
        bundles.keys().cloned().collect()
    }

    #[allow(dead_code)]
    pub fn t<S>(key: &str, args: Option<&[(&str, S)]>, locale: Option<&str>) -> String
    where
        S: ToString + Clone,
    {
        let locale = locale.map(|l| l.to_string()).unwrap_or_else(Self::locale);
        let bundle = get_bundle(&locale);

        let mut fluent_args = FluentArgs::new();
        if let Some(arg_list) = args {
            for &(name, ref value) in arg_list {
                fluent_args.set(name, FluentValue::String(value.to_string().into()));
            }
        }

        // println!("fluent_args: {:?}", fluent_args);

        let result = bundle
            .get_message(key)
            .and_then(|msg| msg.value())
            .map(|pattern| {
                bundle
                    .format_pattern(pattern, Some(&fluent_args), &mut vec![])
                    .into_owned()
            })
            .unwrap_or_else(|| key.to_string())
            .replace(
                [
                    // remove character with zero width (@colored)
                    '\u{2068}', '\u{2069}',
                ],
                "",
            );
        result
    }

    #[allow(dead_code)]
    // #[cfg(test)]
    pub fn set_test_translations(translations: HashMap<String, HashMap<String, String>>) {
        let test_bundles = translations
            .into_iter()
            .map(|(lang, messages)| {
                let resource = FluentResource::try_new(
                    messages
                        .into_iter()
                        .map(|(key, value)| format!("{} = {}", key, value))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
                .unwrap();
                let mut bundle = ConcurrentFluentBundle::new_concurrent(vec![lang.parse().unwrap()]);
                bundle.add_resource(resource).unwrap();
                (lang, Arc::new(bundle))
            })
            .collect();

        let mut bundles = BUNDLES.write().unwrap();
        *bundles = test_bundles;
    }
    #[allow(dead_code)]
    // #[cfg(test)]
    pub fn reset_translations() {
        let mut bundles = BUNDLES.write().unwrap();
        *bundles = load_bundles();
    }
}

fn get_bundle(locale: &str) -> Arc<ConcurrentFluentBundle> {
    let bundles = BUNDLES.read().unwrap();
    let requested_locale = locale
        .parse::<LanguageIdentifier>()
        .unwrap_or_else(|_| DEFAULT_LOCALE.parse().unwrap());
    let available_locales: Vec<LanguageIdentifier> = bundles.keys().map(|s| s.parse().unwrap()).collect();
    let default_locale: LanguageIdentifier = DEFAULT_LOCALE.parse().unwrap();

    let negotiated = negotiate_languages(
        &[requested_locale],
        &available_locales,
        Some(&default_locale),
        NegotiationStrategy::Filtering,
    );

    let chosen_locale = negotiated[0].to_string();
    bundles.get(&chosen_locale).cloned().unwrap_or_else(|| {
        bundles
            .get(DEFAULT_LOCALE)
            .cloned()
            .expect("Default language bundle not found")
    })
}

pub mod macros {
    #[macro_export] // global macro
    macro_rules! __t {
        // Only key
      ($key:expr) => {
          $crate::i18n::I18n::t($key, None::<&[(&str, &str)]>, None)
      };
      // Key and locale
      ($key:expr; $locale:expr) => {
          $crate::i18n::I18n::t($key, None::<&[(&str, &str)]>, Some($locale))
      };
      // Key and arguments
      ($key:expr, $($arg_name:expr => $arg_value:expr),+) => {{
          let args = &[$(($arg_name, $arg_value)),+];
          $crate::i18n::I18n::t($key, Some(args), None)
      }};
      // Key, arguments, and locale
      ($key:expr, $($arg_name:expr => $arg_value:expr),+ $(,)?; $locale:expr) => {{
          let args = &[$(($arg_name, $arg_value)),+];
          $crate::i18n::I18n::t($key, Some(args), Some($locale))
      }};
    }
    // for: use crate::i18n::macros::t;
    // pub(crate) use t;
    pub use crate::__t as t;
}

// for: use jenkins::i18n::t;
#[allow(unused_imports)]
pub use self::macros::t;
