//! Host-testable form helpers shared by the playground shell.

pub fn other_form(form: &str) -> &'static str {
    if form == "protobuf" {
        "yaml"
    } else {
        "protobuf"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_yaml_and_protobuf() {
        assert_eq!(other_form("protobuf"), "yaml");
        assert_eq!(other_form("yaml"), "protobuf");
        assert_eq!(other_form("anything"), "protobuf");
    }
}
