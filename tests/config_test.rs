#[cfg(test)]
mod tests {

    use std::fs;
    use std::path::Path;

    #[test]
    fn test_config_parsing() {
        // This test ensures that our config.yaml file can be parsed correctly
        let config_path = Path::new("config.yaml");
        assert!(config_path.exists(), "config.yaml file not found");

        let config_content = fs::read_to_string(config_path).expect("Failed to read config file");
        let config: serde_yaml::Value =
            serde_yaml::from_str(&config_content).expect("Failed to parse config file");

        // Check that the config has the expected structure
        assert!(
            config.get("roots").is_some(),
            "Config missing 'roots' section"
        );
        assert!(
            config.get("rules").is_some(),
            "Config missing 'rules' section"
        );

        // Check that roots is an array
        let _roots = config["roots"]
            .as_sequence()
            .expect("'roots' is not an array");

        // Check that rules is an array
        let rules = config["rules"]
            .as_sequence()
            .expect("'rules' is not an array");

        // Check that each rule has the required fields
        for (i, rule) in rules.iter().enumerate() {
            assert!(
                rule.get("name").is_some(),
                "Rule {} missing 'name' field",
                i
            );
            assert!(
                rule.get("file_match").is_some(),
                "Rule {} missing 'file_match' field",
                i
            );
            assert!(
                rule.get("exclusions").is_some(),
                "Rule {} missing 'exclusions' field",
                i
            );

            // Check that exclusions is an array
            let _exclusions = rule["exclusions"]
                .as_sequence()
                .unwrap_or_else(|| panic!("Rule {} 'exclusions' is not an array", i));
        }
    }
}
