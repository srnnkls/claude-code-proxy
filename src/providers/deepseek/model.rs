pub const MODEL_PREFIX: &str = "deepseek/";

pub fn advertised_models() -> Vec<String> {
    advertised_models_for(&crate::config::deepseek_models())
}

pub fn advertised_models_for(models: &[String]) -> Vec<String> {
    models
        .iter()
        .map(|model| format!("{MODEL_PREFIX}{model}"))
        .collect()
}

pub fn resolve<'a>(model: &'a str, models: &[String]) -> Option<&'a str> {
    let wire_model = model.strip_prefix(MODEL_PREFIX)?;
    models
        .iter()
        .any(|candidate| candidate == wire_model)
        .then_some(wire_model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_models_require_the_provider_prefix() {
        let models = vec!["deepseek-flash".to_string()];
        assert_eq!(
            resolve("deepseek/deepseek-flash", &models),
            Some("deepseek-flash")
        );
        assert_eq!(resolve("deepseek-flash", &models), None);
        assert_eq!(resolve("opencode-go/deepseek-flash", &models), None);
    }
}
