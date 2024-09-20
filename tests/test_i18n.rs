use jenkins::i18n::t;
use jenkins::i18n::I18n;
use std::collections::HashMap;

#[test]
fn test_i18n_translation() {
    let mut translations = HashMap::new();

    let mut en_us = HashMap::new();
    en_us.insert("hello-world".to_string(), "Hello, world!".to_string());
    en_us.insert("welcome".to_string(), "Welcome, {$name}!".to_string());

    let mut zh_cn = HashMap::new();
    zh_cn.insert("hello-world".to_string(), "你好，世界！".to_string());
    zh_cn.insert("welcome".to_string(), "欢迎，{$name}！".to_string());

    translations.insert("en-US".to_string(), en_us);
    translations.insert("zh-CN".to_string(), zh_cn);

    I18n::set_test_translations(translations); // set mock

    // Test default locale
    assert_eq!(I18n::locale(), "en-US");

    // Test setting locale
    I18n::set_locale("zh-CN");
    assert_eq!(I18n::locale(), "zh-CN");

    // Test basic translation
    assert_eq!(t!("hello-world"), "你好，世界！");
    // println!("{}", t!("hello-world"));

    // Test translation with parameters
    assert_eq!(remove_bidi_isolates(&t!("welcome", "name" => "张三")), "欢迎，张三！");
    println!("{}", t!("welcome", "name" => "张三"));

    // Test translation with specified locale
    assert_eq!(t!("hello-world"; "en-US"), "Hello, world!");

    // Test non-existent locale (should fallback to default language)
    assert_eq!(t!("hello-world"; "Not-Exist"), "Hello, world!");
    println!("{}", t!("hello-world"; "Not-Exist"));

    // Reset translations
    I18n::reset_translations();
}

// Remove fluent's bidi isolates
fn remove_bidi_isolates(s: &str) -> String {
    s.replace(['\u{2068}', '\u{2069}'], "")
}
