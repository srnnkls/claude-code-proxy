use std::collections::BTreeMap;

pub const MODEL_PREFIX: &str = "deepseek/";

pub fn advertised_models() -> Vec<String> {
    advertised_models_for(
        &crate::config::deepseek_models(),
        &crate::config::deepseek_aliases(),
    )
}

pub fn advertised_models_for(models: &[String], aliases: &BTreeMap<String, String>) -> Vec<String> {
    let mut advertised: Vec<String> = models
        .iter()
        .map(|model| format!("{MODEL_PREFIX}{model}"))
        .collect();
    advertised.extend(
        aliases
            .iter()
            .filter(|(_, target)| models.iter().any(|model| model == *target))
            .map(|(alias, _)| alias.clone()),
    );
    advertised
}

pub fn resolve<'a>(
    model: &'a str,
    models: &[String],
    aliases: &'a BTreeMap<String, String>,
) -> Option<&'a str> {
    if let Some(wire_model) = model.strip_prefix(MODEL_PREFIX) {
        return models
            .iter()
            .any(|candidate| candidate == wire_model)
            .then_some(wire_model);
    }
    aliases
        .get(model)
        .filter(|target| models.iter().any(|candidate| candidate == *target))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_models_require_prefix_or_configured_alias() {
        let models = vec!["deepseek-flash".to_string()];
        let aliases = BTreeMap::from([
            ("deepseek-flash".to_string(), "deepseek-flash".to_string()),
            ("ds-flash".to_string(), "deepseek-flash".to_string()),
            ("ds-missing".to_string(), "deepseek-missing".to_string()),
        ]);
        assert_eq!(
            resolve("deepseek/deepseek-flash", &models, &aliases),
            Some("deepseek-flash")
        );
        assert_eq!(
            resolve("ds-flash", &models, &aliases),
            Some("deepseek-flash")
        );
        assert_eq!(resolve("ds-missing", &models, &aliases), None);
        assert_eq!(
            resolve("deepseek-flash", &models, &aliases),
            Some("deepseek-flash")
        );
        assert_eq!(
            resolve("opencode-go/deepseek-flash", &models, &aliases),
            None
        );
    }

    #[test]
    fn aliases_are_advertised_only_for_configured_targets() {
        let models = vec!["deepseek-flash".to_string()];
        let aliases = BTreeMap::from([
            ("deepseek-flash".to_string(), "deepseek-flash".to_string()),
            ("ds-flash".to_string(), "deepseek-flash".to_string()),
            ("ds-missing".to_string(), "deepseek-missing".to_string()),
        ]);
        assert_eq!(
            advertised_models_for(&models, &aliases),
            ["deepseek/deepseek-flash", "deepseek-flash", "ds-flash"]
        );
    }
}
